/// Generate a product type, its result-domain counterpart, and their lattice
/// operations from a single field list using standard Rust type syntax.
///
/// Each field's result-domain type is `<Type as Lift>::Result`, so a field
/// joins the way its type [`Lift`](crate::lattice::lift::Lift)s: a
/// [`PartialJoin`](crate::PartialJoin) scalar as a
/// [`JoinResult`](crate::JoinResult), and `Option`, `BTreeMap` and `HashMap` by lifting
/// their contents. Paths and type aliases resolve like any other type.
///
/// Mirrored fields are private, with public getters and setters by default.
/// Prefix a field with `[accessors(pub(crate))]` to restrict its accessors.
/// Fields of `$Result` are `pub(crate)`.
/// `=> Variant` names its conflict variant. Container fields also name the
/// conflicted value type in parentheses and, for maps, the key type in brackets.
///
/// Generates:
/// 1. `$Source` struct with the fields as written
/// 2. `$Result` struct with the lifted field types
/// 3. `$Source::wrap`
/// 4. `Join for $Result`
/// 5. `$Result::is_ok` and `$Result::try_unwrap`
/// 6. `Lift for $Source` and `ResultWrapped for $Result`
/// 7. Typed conflict paths and `Conflicts for $Result`
///
/// Each entry in `fields` mirrors the field of the same name in the `v2` type.
/// Each entry in `proprietary` is written
/// `name: Type = Codec => Variant`, with the same variant annotations as
/// `fields`. It is stored in a proprietary key of `v2` by the
/// [`ProprietaryField`](crate::psbt::proprietary::ProprietaryField) impl
/// for `Codec`. The struct then has private conversions:
///
/// - `from_v2_fields(v2, proprietary...)` moves the mirrored fields out of
///   `v2` and takes the proprietary fields as given;
/// - `decode_v2(v2)` decodes the proprietary fields in order, removing their
///   keys, then moves the mirrored fields;
/// - `encode_v2(self, base)` moves the mirrored fields into `base` and
///   encodes the proprietary fields into its proprietary keys. Fields of
///   `base` that are not mirrored are kept.
macro_rules! joinable_struct {
    (
        $(#[$source_meta:meta])*
        source: $Source:ident,
        $(#[$result_meta:meta])*
        result: $Result:ident,
        conflicts: $Conflict:ident, $Field:ident,
        v2: $V2:path,
        fields: {
            $(
                $(#[$field_meta:meta])*
                $( [accessors($accessor_vis:vis)] )?
                $field:ident : $T:ty
                => $Variant:ident $( [ $Key:ty ] )? $( ( $Value:ty ) )?
            ),+ $(,)?
        },
        proprietary: {
            $(
                $(#[$proprietary_meta:meta])* $proprietary_vis:vis
                $proprietary:ident : $P:ty = $Codec:ty
                => $ProprietaryVariant:ident $( [ $ProprietaryKey:ty ] )? $( ( $ProprietaryValue:ty ) )?
            ),+ $(,)?
        } $(,)?
    ) => {
        joinable_struct! {
            $(#[$source_meta])*
            source: $Source,
            $(#[$result_meta])*
            result: $Result,
            conflicts: $Conflict, $Field,
            fields: {
                $( $(#[$field_meta])* $field: $T => $Variant $( [ $Key ] )? $( ( $Value ) )?, )+
                $( $(#[$proprietary_meta])* $proprietary_vis $proprietary: $P => $ProprietaryVariant $( [ $ProprietaryKey ] )? $( ( $ProprietaryValue ) )?, )+
            }
        }

        impl $Source {
            $( joinable_struct!(@accessors $(($accessor_vis))? $field: $T); )+
        }

        impl $Source {
            /// The mirrored fields of `v2`, with these proprietary fields.
            fn from_v2_fields(v2: $V2, $( $proprietary: $P ),+) -> Self {
                Self {
                    $( $field: v2.$field, )+
                    $( $proprietary, )+
                }
            }

            /// Decode the proprietary fields of `v2`, removing their keys, and
            /// move its mirrored fields.
            ///
            /// # Errors
            /// Returns the error of the first proprietary field that does not
            /// decode.
            fn decode_v2<E>(mut v2: $V2) -> Result<Self, E>
            where
                $( E: From<<$Codec as $crate::psbt::proprietary::ProprietaryField>::Error>, )+
            {
                $(
                    let $proprietary =
                        <$Codec as $crate::psbt::proprietary::ProprietaryField>::decode(
                            v2.proprietaries.remove(&$crate::psbt::proprietary::key(
                                <$Codec as $crate::psbt::proprietary::ProprietaryField>::SUBTYPE,
                            )),
                        )?;
                )+
                Ok(Self::from_v2_fields(v2, $( $proprietary ),+))
            }

            /// Move the mirrored fields into `base`, and encode the proprietary
            /// fields into its proprietary keys.
            ///
            /// If a proprietary field encodes to `None`, its key is removed from
            /// `base`, even if `self.proprietaries` has it.
            fn encode_v2(self, mut base: $V2) -> $V2 {
                $( base.$field = self.$field; )+
                $(
                    let key = $crate::psbt::proprietary::key(
                        <$Codec as $crate::psbt::proprietary::ProprietaryField>::SUBTYPE,
                    );
                    base.proprietaries.remove(&key);
                    base.proprietaries.extend(
                        <$Codec as $crate::psbt::proprietary::ProprietaryField>::encode(
                            self.$proprietary,
                        )
                        .map(|value| (key, value)),
                    );
                )+
                base
            }
        }
    };

    (
        $(#[$source_meta:meta])*
        source: $Source:ident,
        $(#[$result_meta:meta])*
        result: $Result:ident,
        conflicts: $Conflict:ident, $Field:ident,
        fields: {
            $( $(#[$field_meta:meta])* $vis:vis $field:ident : $T:ty
                => $Variant:ident $( [ $Key:ty ] )? $( ( $Value:ty ) )? ),+ $(,)?
        }
    ) => {
        $(#[$source_meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $Source {
            $(
                $(#[$field_meta])*
                $vis $field: $T,
            )+
        }

        $(#[$result_meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $Result {
            $(
                pub(crate) $field: <$T as $crate::lattice::lift::Lift>::Result,
            )+
        }

        impl $Source {
            #[doc = concat!(
                "Lift into the result domain as a conflict-free [`",
                stringify!($Result),
                "`]."
            )]
            pub fn wrap(self) -> $Result {
                $crate::lattice::lift::Lift::wrap(self)
            }
        }

        impl $crate::lattice::lift::Lift for $Source {
            type Result = $Result;

            fn wrap(self) -> $Result {
                $Result {
                    $(
                        $field: $crate::lattice::lift::Lift::wrap(self.$field),
                    )+
                }
            }
        }

        impl $crate::lattice::join::Join for $Result {
            fn join(self, other: Self) -> Self {
                $Result {
                    $(
                        $field: self.$field.join(other.$field),
                    )+
                }
            }
        }

        impl $Result {
            /// Returns `true` if no field holds a conflict.
            pub fn is_ok(&self) -> bool {
                $(
                    $crate::lattice::lift::ResultWrapped::is_ok(&self.$field)
                )&&+
            }

            /// Extract the clean value, or return `self` if any field holds a
            /// conflict.
            pub fn try_unwrap(self) -> Result<$Source, Self> {
                if !self.is_ok() {
                    return Err(self);
                }
                Ok($Source {
                    $(
                        $field: $crate::lattice::lift::ResultWrapped::try_unwrap(self.$field)
                            .expect("verified all fields are Ok"),
                    )+
                })
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $Field<'a> {
            $( $Variant $( (&'a $Key) )?, )+
        }

        #[derive(Debug, Clone, Copy)]
        pub enum $Conflict<'a> {
            $( $Variant { $( key: &'a $Key, )? values: &'a $crate::Conflict<joinable_struct!(@conflict_value $T $(, $Value)?)> }, )+
        }

        impl<'a> $Conflict<'a> {
            pub fn path(&self) -> $Field<'a> {
                match self {
                    $( Self::$Variant { $( key: joinable_struct!(@conflict_key $Key, key), )? .. } => {
                        joinable_struct!(@conflict_path $Field $Variant key $( [ $Key ] )?)
                    }, )+
                }
            }
        }

        impl $Result {
            /// Borrow each conflicted field, retaining its keydata and value type.
            pub fn conflicts(&self) -> impl Iterator<Item = $Conflict<'_>> {
                // Exhaustive destructuring requires a mapping for every field.
                let Self { $( $field, )+ } = self;
                std::iter::empty()
                    $( .chain($crate::lattice::conflicts::Conflicts::conflicts($field)
                        .map(joinable_struct!(@conflict_item $Conflict $Variant $( [ $Key ] )?))) )+
            }
        }

        impl $crate::lattice::conflicts::Conflicts for $Result {
            type Item<'a> = $Conflict<'a>;

            fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
                Self::conflicts(self)
            }
        }

        impl $crate::lattice::lift::ResultWrapped for $Result {
            type Clean = $Source;

            fn is_ok(&self) -> bool {
                $Result::is_ok(self)
            }

            fn try_unwrap(self) -> Result<$Source, Self> {
                $Result::try_unwrap(self)
            }
        }
    };
    (@accessors $field:ident: $T:ty) => {
        joinable_struct!(@accessors (pub) $field: $T);
    };
    (@accessors ($vis:vis) $field:ident: $T:ty) => {
        pastey::paste! {
            #[doc = concat!("The `", stringify!($field), "` field.")]
            $vis fn $field(&self) -> &$T {
                &self.$field
            }

            #[doc = concat!("Replace the `", stringify!($field), "` field.")]
            $vis fn [<set_ $field>](&mut self, $field: $T) {
                self.$field = $field;
            }
        }
    };

    (@conflict_key $Key:ty, $key:ident) => { $key };
    (@conflict_path $Field:ident $Variant:ident $key:ident [ $Key:ty ]) => { $Field::$Variant(*$key) };
    (@conflict_path $Field:ident $Variant:ident $key:ident) => { $Field::$Variant };
    (@conflict_item $Conflict:ident $Variant:ident [ $Key:ty ]) => {
        |(key, values)| $Conflict::$Variant { key, values }
    };
    (@conflict_item $Conflict:ident $Variant:ident) => {
        |values| $Conflict::$Variant { values }
    };
    (@conflict_value $T:ty) => { $T };
    (@conflict_value $T:ty, $Value:ty) => { $Value };

}

// Outside the `coverage(off)` test module, so that this expansion of the
// macro counts towards coverage.
#[cfg(all(test, feature = "unit-tests"))]
mod sample {
    use std::collections::BTreeMap;

    joinable_struct! {
        /// One field per kind of lifting, the option spelled as a path.
        source: Sample,
        /// Result-domain form of [`Sample`].
        result: ResultSample,
        conflicts: SampleConflict, SampleField,
        fields: {
            pub scalar: u8 => Scalar,
            pub option: std::option::Option<u8> => Option(u8),
            pub map: BTreeMap<u8, u8> => Map[u8](u8),
        }
    }

    impl Sample {
        joinable_struct!(@accessors scalar: u8);
        joinable_struct!(@accessors (pub(crate)) option: std::option::Option<u8>);
    }
}

#[cfg(all(test, feature = "unit-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::BTreeMap;

    use super::sample::Sample;
    use crate::lattice::join::Join;
    use crate::lattice::lift::{Lift, ResultWrapped};

    fn sample(scalar: u8, option: u8, value: u8) -> Sample {
        Sample {
            scalar,
            option: Some(option),
            map: BTreeMap::from([(0, value)]),
        }
    }

    #[test]
    fn default_and_restricted_accessors_read_and_update_fields() {
        let mut value = sample(1, 2, 3);
        assert_eq!(value.scalar(), &1);
        assert_eq!(value.option(), &Some(2));
        value.set_scalar(4);
        value.set_option(None);
        assert_eq!(value.scalar(), &4);
        assert_eq!(value.option(), &None);
    }

    #[test]
    fn structs_lift_inside_a_map() {
        let clean = std::collections::HashMap::from([(0u8, sample(1, 2, 3))]);
        let lifted = clean.wrap();
        assert_eq!(lifted[&0], sample(1, 2, 3).wrap());
        assert_eq!(
            ResultWrapped::try_unwrap(lifted).ok(),
            Some(std::collections::HashMap::from([(0u8, sample(1, 2, 3))]))
        );
    }

    #[test]
    fn equal_structs_join_and_unwrap() {
        let joined = sample(1, 2, 3).wrap().join(sample(1, 2, 3).wrap());
        assert!(ResultWrapped::is_ok(&joined));
        assert_eq!(
            ResultWrapped::try_unwrap(joined).ok(),
            Some(sample(1, 2, 3))
        );
    }

    #[test]
    fn conflicting_fields_retain_typed_paths() {
        let joined = sample(1, 2, 3).wrap().join(sample(4, 5, 6).wrap());
        assert!(!joined.is_ok());
        use super::sample::SampleField;
        assert_eq!(
            joined.conflicts().map(|c| c.path()).collect::<Vec<_>>(),
            [
                SampleField::Scalar,
                SampleField::Option,
                SampleField::Map(&0)
            ]
        );
        use super::sample::SampleConflict;
        for (conflict, expected) in joined.conflicts().zip([[1, 4], [2, 5], [3, 6]]) {
            match conflict {
                SampleConflict::Scalar { values }
                | SampleConflict::Option { values }
                | SampleConflict::Map { values, .. } => {
                    assert_eq!(values.into_iter().copied().collect::<Vec<_>>(), expected);
                }
            }
        }
        assert_eq!(ResultWrapped::try_unwrap(joined.clone()), Err(joined));
    }
}
