mod spec;
mod status;

pub use self::spec::*;
pub use self::status::*;
pub use custom_metadata::CustomSpuKey;

#[cfg(feature = "k8")]
mod k8;

mod metadata {

    use crate::core::{Spec, Status};
    use crate::spg::SpuGroupSpec;
    use crate::extended::{ObjectType, SpecExt};

    use super::*;

    impl Spec for SpuSpec {
        const LABEL: &'static str = "SPU";
        type IndexKey = String;
        type Owner = SpuGroupSpec;
        type Status = SpuStatus;
    }

    impl SpecExt for SpuSpec {
        const OBJECT_TYPE: ObjectType = ObjectType::Spu;
    }

    impl Status for SpuStatus {}

    #[cfg(feature = "k8")]
    mod extended {

        use crate::store::k8::{K8ExtendedSpec, K8ConvertError, K8MetaItem};
        use crate::store::MetadataStoreObject;
        use crate::k8_types::K8Obj;
        use crate::store::k8::default_convert_from_k8;

        use super::SpuSpec;

        impl K8ExtendedSpec for SpuSpec {
            type K8Spec = Self;

            fn convert_from_k8(
                k8_obj: K8Obj<Self::K8Spec>,
                multi_namespace_context: bool,
            ) -> Result<MetadataStoreObject<Self, K8MetaItem>, K8ConvertError<Self::K8Spec>>
            {
                default_convert_from_k8(k8_obj, multi_namespace_context)
            }

            fn convert_status_from_k8(status: Self::Status) -> Self::Status {
                status
            }

            fn into_k8(self) -> Self::K8Spec {
                self
            }
        }
    }
}

mod custom_metadata {

    use std::io::Error;
    use std::io::ErrorKind;
    use std::fmt;

    use tracing::trace;

    use fluvio_protocol::Encoder;
    use fluvio_protocol::Decoder;
    use fluvio_protocol::Version;
    use fluvio_protocol::bytes::{Buf, BufMut};

    use crate::core::{Spec, Removable, Creatable};
    use crate::extended::{ObjectType, SpecExt};

    use super::*;

    /// this is not real spec but is there to allow passing of parameters
    impl Spec for CustomSpuSpec {
        const LABEL: &'static str = "CustomSpu";
        type IndexKey = String;
        type Status = SpuStatus;
        type Owner = SpuSpec;
    }

    impl SpecExt for CustomSpuSpec {
        const OBJECT_TYPE: ObjectType = ObjectType::CustomSpu;
    }

    impl Removable for CustomSpuSpec {
        type DeleteKey = CustomSpuKey;
    }

    impl Creatable for CustomSpuSpec {}

    // This can be auto generated by enum derive later
    #[derive(Debug)]
    pub enum CustomSpuKey {
        Name(String),
        Id(i32),
    }

    impl fmt::Display for CustomSpuKey {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                Self::Name(name) => write!(f, "{name}"),
                Self::Id(id) => write!(f, "{id}"),
            }
        }
    }

    impl From<&CustomSpuKey> for String {
        fn from(key: &CustomSpuKey) -> Self {
            match key {
                CustomSpuKey::Name(name) => name.to_owned(),
                CustomSpuKey::Id(id) => id.to_string(),
            }
        }
    }

    impl CustomSpuKey {
        fn type_string(&self) -> &'static str {
            match self {
                Self::Name(_) => "Name",
                Self::Id(_) => "Id",
            }
        }
    }

    impl Default for CustomSpuKey {
        fn default() -> Self {
            Self::Id(0)
        }
    }

    impl Encoder for CustomSpuKey {
        fn write_size(&self, version: Version) -> usize {
            let type_size = self.type_string().to_owned().write_size(version);

            type_size
                + match self {
                    Self::Name(s) => s.write_size(version),
                    Self::Id(s) => s.write_size(version),
                }
        }

        // encode match
        fn encode<T>(&self, dest: &mut T, version: Version) -> Result<(), Error>
        where
            T: BufMut,
        {
            self.type_string().to_owned().encode(dest, version)?;

            match self {
                Self::Name(s) => s.encode(dest, version)?,
                Self::Id(s) => s.encode(dest, version)?,
            }

            Ok(())
        }
    }

    impl Decoder for CustomSpuKey {
        fn decode<T>(&mut self, src: &mut T, version: Version) -> Result<(), Error>
        where
            T: Buf,
        {
            let mut typ = "".to_owned();
            typ.decode(src, version)?;
            trace!("decoded type: {}", typ);

            match typ.as_ref() {
                "Name" => {
                    let mut response = String::default();
                    response.decode(src, version)?;
                    *self = Self::Name(response);
                    Ok(())
                }

                "Id" => {
                    let mut response: i32 = 9;
                    response.decode(src, version)?;
                    *self = Self::Id(response);
                    Ok(())
                }

                // Unexpected type
                _ => Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("invalid spec type {typ}"),
                )),
            }
        }
    }
}
