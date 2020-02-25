
use self::super::{Versionize, VersionMap};


macro_rules! primitive_versionize {
    ($ty:ident) => {
        impl Versionize for $ty {
            #[inline]
            fn serialize<W: std::io::Write>(&self, writer: &mut W, _version_map: &VersionMap, _version: u16) {
                bincode::serialize_into(writer, &self).unwrap();
            }
            #[inline]
            fn deserialize<R: std::io::Read>(mut reader: &mut R, _version_map: &VersionMap, _version: u16) -> Self {
                bincode::deserialize_from(&mut reader).unwrap()
            }

            // Not used.
            fn name() -> String {
                String::new()
            }
            // Not used.
            fn version() -> u16 {
                1
            }
        }
    };
}

primitive_versionize!(bool);
primitive_versionize!(isize);
primitive_versionize!(i8);
primitive_versionize!(i16);
primitive_versionize!(i32);
primitive_versionize!(i64);
primitive_versionize!(usize);
primitive_versionize!(u8);
primitive_versionize!(u16);
primitive_versionize!(u32);
primitive_versionize!(u64);
primitive_versionize!(f32);
primitive_versionize!(f64);
primitive_versionize!(char);
primitive_versionize!(String);
// primitive_versionize!(Option<T>);

#[cfg(feature = "std")]
primitive_versionize!(CStr);
#[cfg(feature = "std")]
primitive_versionize!(CString);

impl<T> Versionize for Vec<T>
where
    T: Versionize,
{
    #[inline]
    fn serialize<W: std::io::Write>(&self, mut writer: &mut W, version_map: &VersionMap, app_version: u16) {
        // Serialize in the same fashion as bincode:
        // len, T, T, ...
        bincode::serialize_into(&mut writer, &self.len()).unwrap();
        for obj in self {
            obj.serialize(writer, version_map, app_version);
        }
    }

    #[inline]
    fn deserialize<R: std::io::Read>(mut reader: &mut R, version_map: &VersionMap, app_version: u16) -> Self {
        let mut v = Vec::new();
        let len: u64 = bincode::deserialize_from(&mut reader).unwrap();
        for _ in 0..len {
            let obj: T = T::deserialize(reader, version_map, app_version);
            v.push(obj);
        }
        v
    }

    // Not used.
    fn name() -> String {
        String::new()
    }

    // Not used.
    fn version() -> u16 {
        1
    }
}