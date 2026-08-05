macro_rules! closed_set {
    (
        $(#[$set_doc:meta])*
        enum $set:ident;
        $(#[$all_doc:meta])*
        const ALL;
        $($(#[$variant_doc:meta])* $variant:ident,)+
    ) => {
        $(#[$set_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $set {
            $($(#[$variant_doc])* $variant,)+
        }

        impl $set {
            $(#[$all_doc])*
            pub const ALL: [Self; [$(stringify!($variant)),+].len()] = [$(Self::$variant),+];
        }
    };
}

pub(crate) use closed_set;
