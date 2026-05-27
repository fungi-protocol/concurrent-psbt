/// Generate a result-domain product type and its lattice operations from a
/// single field list using standard Rust type syntax.
///
/// Fields are classified automatically:
///
/// - `name: Type`                  → scalar → `name: JoinResult<Type>`
/// - `name: Option<Type>`          → option → `name: Option<JoinResult<Type>>`
/// - `name: BTreeMap<K, V>`        → map    → `name: BTreeMap<K, JoinResult<V>>`
///
/// Generates:
/// 1. `$Result` struct with transformed field types
/// 2. `$Ext` trait with `wrap` method on the source type
/// 3. `Join for $Result`
/// 4. `$Result::is_ok`
/// 5. `$Result::try_unwrap`
#[allow(unused_macros)]
macro_rules! joinable_struct {
    // ── Entry point ──────────────────────────────────────────
    (
        $(#[$result_meta:meta])*
        source: $Source:ident,
        result: $Result:ident,
        ext: $Ext:ident,
        fields: { $($fields:tt)* }
    ) => {
        joinable_struct!(@munch
            meta: [ $(#[$result_meta])* ]
            source: $Source
            result: $Result
            ext: $Ext
            classified: []
            rest: [ $($fields)* ]
        );
    };

    // ── Option<T> ────────────────────────────────────────────
    (@munch
        meta: $meta:tt
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $($classified:tt)* ]
        rest: [ $field:ident : Option < $T:ty > , $($rest:tt)* ]
    ) => {
        joinable_struct!(@munch
            meta: $meta
            source: $Source
            result: $Result
            ext: $Ext
            classified: [ $($classified)* (option $field $T) ]
            rest: [ $($rest)* ]
        );
    };

    // ── BTreeMap<K, V> ──────────────────────────────────────
    (@munch
        meta: $meta:tt
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $($classified:tt)* ]
        rest: [ $field:ident : BTreeMap < $K:ty , $V:ty > , $($rest:tt)* ]
    ) => {
        joinable_struct!(@munch
            meta: $meta
            source: $Source
            result: $Result
            ext: $Ext
            classified: [ $($classified)* (map $field [ $K => $V ]) ]
            rest: [ $($rest)* ]
        );
    };

    // ── Scalar T (catch-all; must come after Option, BTreeMap) ─
    (@munch
        meta: $meta:tt
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $($classified:tt)* ]
        rest: [ $field:ident : $T:ty , $($rest:tt)* ]
    ) => {
        joinable_struct!(@munch
            meta: $meta
            source: $Source
            result: $Result
            ext: $Ext
            classified: [ $($classified)* (scalar $field $T) ]
            rest: [ $($rest)* ]
        );
    };

    // ── Terminal: no more fields ─────────────────────────────
    (@munch
        meta: [ $(#[$result_meta:meta])* ]
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $( ($category:ident $field:ident $type_tokens:tt) )* ]
        rest: []
    ) => {
        // 1. Result struct
        $(#[$result_meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $Result {
            $(
                pub(crate) $field: joinable_struct!(@result_type $category $type_tokens),
            )*
        }

        // 2. Ext trait + impl
        pub trait $Ext {
            fn wrap(self) -> $Result;
        }

        impl $Ext for $Source {
            fn wrap(self) -> $Result {
                $Result {
                    $(
                        $field: joinable_struct!(@wrap $category self . $field),
                    )*
                }
            }
        }

        // 3. Join
        impl $crate::lattice::join::Join for $Result {
            fn join(self, other: Self) -> Self {
                $Result {
                    $(
                        $field: self.$field.join(other.$field),
                    )*
                }
            }
        }

        // 4. is_ok + 5. try_unwrap
        impl $Result {
            pub fn is_ok(&self) -> bool {
                $(
                    joinable_struct!(@is_ok $category self . $field)
                )&&*
            }

            /// Visit each conflicted field, calling `f(field_name, &conflict)`.
            ///
            /// Only visits fields that contain conflicts. Clean fields are skipped.
            /// For map fields, visits each conflicted entry with the field name.
            #[allow(dead_code)]
            pub(crate) fn for_each_conflict(
                &self,
                mut f: impl FnMut(&str, &dyn std::fmt::Debug),
            ) {
                $(
                    joinable_struct!(@visit_conflict $category self . $field, f);
                )*
            }

            #[allow(clippy::result_large_err)]
            pub fn try_unwrap(self) -> Result<$Source, Self> {
                if !self.is_ok() {
                    return Err(self);
                }
                Ok($Source {
                    $(
                        $field: joinable_struct!(@unwrap $category self . $field),
                    )*
                })
            }
        }
    };

    // --- Type transformation rules ---

    (@result_type scalar $T:ty) => {
        $crate::lattice::partial::JoinResult<$T>
    };
    (@result_type option $T:ty) => {
        Option<$crate::lattice::partial::JoinResult<$T>>
    };
    (@result_type map [$K:ty => $V:ty]) => {
        std::collections::BTreeMap<$K, $crate::lattice::partial::JoinResult<$V>>
    };

    // --- Wrap rules ---

    (@wrap scalar $self:ident . $field:ident) => {
        $crate::lattice::partial::PartialJoin::wrap($self.$field)
    };
    (@wrap option $self:ident . $field:ident) => {
        $crate::collections::option::OptionExt::wrap($self.$field)
    };
    (@wrap map $self:ident . $field:ident) => {
        $crate::collections::btreemap::BTreeMapExt::wrap($self.$field)
    };

    // --- is_ok rules ---

    (@is_ok scalar $self:ident . $field:ident) => {
        $self.$field.is_ok()
    };
    (@is_ok option $self:ident . $field:ident) => {
        $crate::collections::option::ResultOptionExt::is_ok(&$self.$field)
    };
    (@is_ok map $self:ident . $field:ident) => {
        $crate::collections::btreemap::ResultBTreeMapExt::is_ok(&$self.$field)
    };

    // --- Visit-conflict rules ---

    (@visit_conflict scalar $self:ident . $field:ident , $f:expr) => {
        if let Err(c) = &$self.$field {
            ($f)(stringify!($field), c as &dyn std::fmt::Debug);
        }
    };
    (@visit_conflict option $self:ident . $field:ident , $f:expr) => {
        if let Some(Err(c)) = &$self.$field {
            ($f)(stringify!($field), c as &dyn std::fmt::Debug);
        }
    };
    (@visit_conflict map $self:ident . $field:ident , $f:expr) => {
        for (_k, v) in &$self.$field {
            if let Err(c) = v {
                ($f)(stringify!($field), c as &dyn std::fmt::Debug);
            }
        }
    };

    // --- Unwrap rules ---

    (@unwrap scalar $self:ident . $field:ident) => {
        $self.$field.expect("verified all fields are Ok")
    };
    (@unwrap option $self:ident . $field:ident) => {
        $crate::collections::option::ResultOptionExt::try_unwrap($self.$field)
            .expect("verified all fields are Ok")
    };
    (@unwrap map $self:ident . $field:ident) => {
        $crate::collections::btreemap::ResultBTreeMapExt::try_unwrap($self.$field)
            .expect("verified all fields are Ok")
    };
}
