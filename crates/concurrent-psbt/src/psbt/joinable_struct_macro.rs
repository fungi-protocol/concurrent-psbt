/// Generate a result-domain product type and its lattice operations from a
/// single field list using standard Rust type syntax.
///
/// Each field's result-domain type is `<Type as Lift>::Result`, so a field
/// joins the way its type [`Lift`](crate::lattice::lift::Lift)s: a
/// [`PartialJoin`](crate::PartialJoin) scalar as a
/// [`JoinResult`](crate::JoinResult), and `Option`, `BTreeMap` and `HashMap` by lifting
/// their contents. Paths and type aliases resolve like any other type.
///
/// Generates:
/// 1. `$Result` struct with the lifted field types
/// 2. `$Ext` trait with `wrap` method on the source type
/// 3. `Join for $Result`
/// 4. `$Result::is_ok` and `$Result::try_unwrap`
/// 5. `Lift for $Source` and `ResultWrapped for $Result`
///
/// The field list must name at least one field: `is_ok` is generated as the
/// conjunction of the per-field checks.
macro_rules! joinable_struct {
    (
        $(#[$result_meta:meta])*
        source: $Source:ident,
        result: $Result:ident,
        conflicts: $Conflict:ident, $Field:ident { $($conflict_fields:tt)* },
        ext: $Ext:ident,
        fields: {
            $( $field:ident : $T:ty ),+ $(,)?
        }
    ) => {
        $(#[$result_meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $Result {
            $(
                pub(crate) $field: <$T as $crate::lattice::lift::Lift>::Result,
            )+
        }

        pub use $crate::Lift as $Ext;

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

        record_conflicts! {
            $Result, $Conflict, $Field { $($conflict_fields)* }
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
}

// Outside the `coverage(off)` test module, so that this expansion of the
// macro counts towards coverage.
#[cfg(all(test, feature = "unit-tests"))]
mod sample {
    use std::collections::BTreeMap;

    /// One field per kind of lifting, the option spelled as a path.
    #[derive(Debug, PartialEq)]
    pub struct Sample {
        pub scalar: u8,
        pub option: std::option::Option<u8>,
        pub map: BTreeMap<u8, u8>,
    }

    joinable_struct! {
        /// Result-domain form of [`Sample`].
        source: Sample,
        result: ResultSample,
        conflicts: SampleConflict, SampleField {
            Scalar => scalar: u8,
            Option => option: u8,
            Map => map[u8]: u8,
        },
        ext: SampleExt,
        fields: {
            scalar: u8,
            option: std::option::Option<u8>,
            map: BTreeMap<u8, u8>,
        }
    }
}

#[cfg(all(test, feature = "unit-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::BTreeMap;

    use super::sample::{Sample, SampleExt};
    use crate::lattice::join::Join;
    use crate::lattice::lift::ResultWrapped;

    fn sample(scalar: u8, option: u8, value: u8) -> Sample {
        Sample {
            scalar,
            option: Some(option),
            map: BTreeMap::from([(0, value)]),
        }
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
