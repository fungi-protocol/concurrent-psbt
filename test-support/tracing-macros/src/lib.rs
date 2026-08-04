//! A `#[test]` that saves a Perfetto trace of each test run.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Like `#[test]`, but writes a trace file to `NEXTEST_TRACE_DIR` when it is set.
#[proc_macro_attribute]
pub fn test(args: TokenStream, item: TokenStream) -> TokenStream {
    parse_macro_input!(args as syn::parse::Nothing);
    let mut function = parse_macro_input!(item as ItemFn);
    let name = &function.sig.ident;
    let body = &function.block;
    // Matching on the recording keeps it alive until the test ends.
    function.block = syn::parse_quote!({
        match ::std::env::var_os("NEXTEST_TRACE_DIR")
            .filter(|directory| !directory.is_empty())
            .map(|directory| {
                const NAME: &str =
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#name));
                // GitHub artifacts reject ':' in file names. The process ID
                // stops a retry from overwriting the first attempt.
                ::std::fs::create_dir_all(&directory).expect("create test trace directory");
                let file = ::std::path::Path::new(&directory).join(::std::format!(
                    "{}.{}.json",
                    NAME.replace("::", "."),
                    ::std::process::id(),
                ));
                let (layer, writer) = ::tracing_chrome::ChromeLayerBuilder::new()
                    .file(file)
                    .include_args(true)
                    .build();
                let subscriber = ::tracing_subscriber::layer::SubscriberExt::with(
                    ::tracing_subscriber::registry(),
                    layer,
                );
                // nextest gives each test its own process, so record the whole
                // process, including spawned threads. `cargo test` shares one
                // process, so record only this thread.
                let subscriber = if ::std::env::var("NEXTEST_EXECUTION_MODE").as_deref()
                    == Ok("process-per-test")
                {
                    ::tracing::subscriber::set_global_default(subscriber)
                        .expect("install test process subscriber");
                    None
                } else {
                    Some(::tracing::subscriber::set_default(subscriber))
                };
                let process = ::tracing::info_span!(parent: None, "process",
                    process.pid = ::std::process::id()).entered();
                let test = ::tracing::info_span!("test", test.name = NAME).entered();
                // Dropped in this order, even on panic: spans close before
                // the file is written, so the file is complete.
                (test, process, subscriber, writer)
            }) {
            _ => #body
        }
    });
    // The full path names the standard attribute; plain `#[test]` would be this macro.
    quote!(
        #[cfg_attr(coverage_nightly, coverage(off))]
        #[::core::prelude::v1::test]
        #function
    )
    .into()
}
