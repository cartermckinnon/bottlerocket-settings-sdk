//! Provides the [`SettingsModel`] trait interface required to model new settings in the
//! Bottlerocket API using the settings SDK.
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt::Debug, marker::PhantomData};

#[doc(hidden)]
pub mod erased;
pub use erased::{AsTypeErasedModel, TypeErasedModel};
pub use error::BottlerocketSettingError;

/// This trait is required to model new settings in the Bottlerocket API using the settings SDK.
///
/// To get started, you can describe the shape ("model") of your data using any struct which
/// implements [`Serialize`](serde::Serialize), [`Deserialize`](serde::Deserialize), and
/// [`Debug`](std::fmt::Debug), and then implement [`SettingsModel`]:
///
/// ```
/// # use anyhow::Result;
/// # use bottlerocket_settings_sdk::{SettingsModel, GenerateResult};
/// # use serde::{Serialize, Deserialize};
/// # use std::convert::Infallible;
///
/// /// Suppose we wish to allow setting our name and favorite number in the API.
/// #[derive(Debug, Serialize, Deserialize, Default)]
/// struct MySettings {
///     name: String,
///     favorite_number: i64,
/// }
///
/// // Implementing `bottlerocket_settings_sdk::SettingsModel` allows the settings SDK to expose
/// // these settings in the Bottlerocket API.
/// impl SettingsModel for MySettings {
///     type PartialKind = Self;
///     type ErrorKind = anyhow::Error;
///
///     fn get_version() -> &'static str {
///         "v1"
///     }
///
///     fn set(current_value: Option<Self>, target: Self) -> Result<Self> {
///         // Perform any additional validations of the new value here...
///         Ok(target)
///     }
///
///     fn generate(
///         _: Option<Self::PartialKind>,
///         _: Option<serde_json::Value>,
///     ) -> Result<GenerateResult<Self::PartialKind, Self>> {
///         // Dynamic generation of the value occurs here...
///         Ok(GenerateResult::Complete(MySettings::default()))
///     }
///
///     fn validate(_value: Self, _validated_settings: Option<serde_json::Value>) -> Result<bool> {
///         // Cross-validation of new values can occur against other settings here...
///         Ok(true)
///     }
/// }
///
/// ```
///
/// Once you have implemented the interface for the model, you must also select
/// [which migrator](crate::migrate) to use, and implement any traits required for that migrator.
pub trait SettingsModel: Sized + Serialize + DeserializeOwned + Debug {
    /// A type that represents a partially-constructed version of the implementor of this trait.
    ///
    /// This is used during settings generation to represent cases in which a user has given an
    /// incomplete version of the data, where more should be generated.
    type PartialKind: Serialize + DeserializeOwned;

    /// The error type returned by the settings extension.
    type ErrorKind: Into<Box<dyn std::error::Error + Send + Sync + 'static>>;

    /// Returns the version of this settings model, e.g. "v1".
    fn get_version() -> &'static str;

    /// Determines whether this setting can be set to the `target` value, given its current value.
    ///
    /// The returned value is what is ultimately set in the settings datastore. While this leaves
    /// room for the extension to modify the value that is stored, this should be done cautiously
    /// so as not to confuse users.
    fn set(current_value: Option<Self>, target: Self) -> Result<Self, Self::ErrorKind>;

    /// Generates default values at system start.
    ///
    /// The settings system repeatedly invokes `generate` on all settings until they have
    /// completed. On each generation cycle, the settings extension is provided any values that it
    /// has previously generated, as well as all of the data that has thus far been generated by its
    /// dependencies.
    fn generate(
        existing_partial: Option<Self::PartialKind>,
        dependent_settings: Option<serde_json::Value>,
    ) -> Result<GenerateResult<Self::PartialKind, Self>, Self::ErrorKind>;

    /// Validates this setting, allowing for cross-validation with other settings.
    ///
    /// Cross-validated settings are provided as a JSON Map, where the key is the extension name and
    /// the value is the value of that setting.
    fn validate(
        _value: Self,
        _validated_settings: Option<serde_json::Value>,
    ) -> Result<bool, Self::ErrorKind>;
}

/// This struct wraps [`SettingsModel`]s in a referencable object which is passed to the
/// [`SettingsExtension`](crate::SettingsExtension) API to represent the model
///
/// ```
/// # use bottlerocket_settings_sdk::example::empty::EmptySetting;
/// # use bottlerocket_settings_sdk::{LinearMigratorExtensionBuilder, LinearMigrator, BottlerocketSetting};
/// # type MySettingV1 = EmptySetting;
/// # type MySettingV2 = EmptySetting;
/// let settings_extension = LinearMigratorExtensionBuilder::with_name("example")
///     .with_models(vec![
///         BottlerocketSetting::<MySettingV1>::model(),
///         BottlerocketSetting::<MySettingV2>::model(),
///     ])
///     .build();
/// ```
#[derive(Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Default)]
pub struct BottlerocketSetting<T: SettingsModel> {
    _ghost: PhantomData<T>,
}

impl<T: SettingsModel> BottlerocketSetting<T> {
    /// Boxes the object so that it can be used in the settings SDK as a `Box<dyn Model>`.
    pub fn model() -> Box<Self> {
        Box::new(Self {
            _ghost: PhantomData,
        })
    }
}

/// The result of generating a setting value at runtime.
///
/// The settings system repeatedly invokes `generate` on all settings until they have
/// completed. On each generation cycle, the settings extension is provided any values that it
/// has previously generated, as well as all of the data that has thus far been generated by its
/// dependencies.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GenerateResult<Partial, Complete> {
    /// Returned during settings generation to signal that other settings are required to generate
    /// more data before this generation can complete.
    NeedsData(Option<Partial>),

    /// Signals that settings generation has completed, returning the underlying data.
    Complete(Complete),
}

impl<P, C> GenerateResult<P, C>
where
    P: Serialize + DeserializeOwned,
    C: Serialize + DeserializeOwned,
{
    /// Serializes the underlying result types into JSON values.
    pub fn serialize(
        self,
    ) -> Result<GenerateResult<serde_json::Value, serde_json::Value>, serde_json::Error> {
        Ok(match self {
            GenerateResult::NeedsData(optional_interior) => GenerateResult::NeedsData(
                optional_interior
                    .map(|i| serde_json::to_value(i))
                    .transpose()?,
            ),
            GenerateResult::Complete(interior) => {
                GenerateResult::Complete(serde_json::to_value(interior)?)
            }
        })
    }
}

mod error {
    #![allow(missing_docs)]
    use snafu::Snafu;

    /// The error type returned when interacting with a user-defined
    /// [`SettingsModel`](crate::SettingsModel).
    #[derive(Debug, Snafu)]
    #[snafu(visibility(pub))]
    pub enum BottlerocketSettingError {
        #[snafu(display(
            "Failed to deserialize '{}' input as settings value version '{}': {}\nValue: {}",
            input_type,
            version,
            source,
            serde_json::to_string_pretty(&input).unwrap_or(input.to_string()),
        ))]
        DeserializeInput {
            input_type: &'static str,
            input: serde_json::Value,
            version: &'static str,
            source: serde_json::Error,
        },

        #[snafu(display(
            "Failed to run 'generate' on setting version '{}': {}",
            version,
            source
        ))]
        GenerateSetting {
            version: &'static str,
            source: Box<dyn std::error::Error + Send + Sync + 'static>,
        },

        #[snafu(display(
            "Failed to parse setting value (version '{}') from JSON: {}",
            version,
            source
        ))]
        ParseSetting {
            version: &'static str,
            source: serde_json::Error,
        },

        #[snafu(display(
            "Failed to serialize settings extension (version '{}') '{}' result: {}",
            version,
            operation,
            source
        ))]
        SerializeResult {
            version: &'static str,
            operation: &'static str,
            source: serde_json::Error,
        },

        #[snafu(display("Failed to run 'set' on setting version '{}': {}", version, source))]
        SetSetting {
            version: &'static str,
            source: Box<dyn std::error::Error + Send + Sync + 'static>,
        },

        #[snafu(display(
            "Failed to run 'validate' on setting version '{}': {}",
            version,
            source
        ))]
        ValidateSetting {
            version: &'static str,
            source: Box<dyn std::error::Error + Send + Sync + 'static>,
        },
    }
}
