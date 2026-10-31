//! Syntactic checks for the pure provider-codec boundary.
//!
//! These checks inspect all platform branches. They do not resolve names or prove
//! that arbitrary macros and indirect calls have no effects. Code review must
//! also check aliases, helpers and dependencies.

use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Attribute, Item, Meta, UseTree};

use crate::Error;

pub(crate) fn check(root: &Path) -> Result<(), Error> {
    let providers = root.join("crates/ai/src/codecs/providers");
    let mut files = Vec::new();
    collect(&providers, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(Error::Policy(
            "provider source directory has no Rust files".into(),
        ));
    }
    let mut failures = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file)?;
        let findings = inspect(&source)
            .map_err(|error| Error::Policy(format!("{}: {error}", file.display())))?;
        for finding in findings {
            failures.push(format!("{}: {finding}", file.display()));
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    Err(Error::Policy(format!(
        "provider codecs must be synchronous data transformations; execution and credentials have separate owners:\n{}",
        failures.join("\n")
    )))
}

fn collect(directory: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), Error> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(Error::Policy(format!(
                "provider source cannot be a symbolic link: {}",
                entry.path().display()
            )));
        }
        if kind.is_dir() {
            collect(&entry.path(), files)?;
        } else if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn inspect(source: &str) -> Result<Vec<String>, Error> {
    let file = syn::parse_file(source)?;
    let mut checker = Boundary {
        findings: Vec::new(),
    };
    checker.visit_file(&file);
    Ok(checker.findings)
}

struct Boundary {
    findings: Vec<String>,
}

impl Boundary {
    fn report(&mut self, span: proc_macro2::Span, rule: &str) {
        self.findings
            .push(format!("line {}: {rule}", span.start().line));
    }

    fn path(&mut self, segments: &[String], span: proc_macro2::Span) {
        let mut segments = segments.iter().map(String::as_str);
        let first = segments.next();
        let second = segments.next();
        let third = segments.next();
        let forbidden = match (first, second, third) {
            (Some("reqwest" | "hyper" | "ureq" | "tokio"), _, _) => true,
            (Some("std"), Some("fs" | "net" | "process" | "thread"), _) => true,
            (Some("std"), Some("env"), Some("consts")) => false,
            (Some("std"), Some("env"), _) => true,
            _ => false,
        };
        if forbidden {
            self.report(
                span,
                "I/O and ambient credentials belong outside provider codecs",
            );
        }
    }

    fn imports(&mut self, prefix: Vec<String>, tree: &UseTree) {
        match tree {
            UseTree::Path(path) => {
                let mut prefix = prefix;
                prefix.push(path.ident.to_string());
                self.imports(prefix, &path.tree);
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.imports(prefix.clone(), item);
                }
            }
            UseTree::Name(name) => {
                let mut prefix = prefix;
                prefix.push(name.ident.to_string());
                self.path(&prefix, name.ident.span());
            }
            UseTree::Rename(rename) => {
                let mut prefix = prefix;
                prefix.push(rename.ident.to_string());
                self.path(&prefix, rename.ident.span());
            }
            UseTree::Glob(glob) => self.path(&prefix, glob.star_token.span),
        }
    }

    fn macro_tokens(&mut self, tokens: TokenStream) {
        let mut dot = false;
        for token in tokens {
            match &token {
                TokenTree::Group(group) => self.macro_tokens(group.stream()),
                TokenTree::Ident(ident) if ident == "async" || (dot && ident == "await") => {
                    self.report(
                        ident.span(),
                        "async execution is not permitted inside provider macros",
                    );
                }
                _ => {}
            }
            dot = matches!(token, TokenTree::Punct(punctuation) if punctuation.as_char() == '.');
        }
    }
}

fn test_only(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && matches!(attribute.parse_args::<Meta>(), Ok(Meta::Path(path)) if path.is_ident("test"))
    })
}

impl<'ast> Visit<'ast> for Boundary {
    fn visit_item(&mut self, item: &'ast Item) {
        let attributes: &[Attribute] = match item {
            Item::Const(item) => &item.attrs,
            Item::Enum(item) => &item.attrs,
            Item::ExternCrate(item) => &item.attrs,
            Item::Fn(item) => &item.attrs,
            Item::ForeignMod(item) => &item.attrs,
            Item::Impl(item) => &item.attrs,
            Item::Macro(item) => &item.attrs,
            Item::Mod(item) => &item.attrs,
            Item::Static(item) => &item.attrs,
            Item::Struct(item) => &item.attrs,
            Item::Trait(item) => &item.attrs,
            Item::TraitAlias(item) => &item.attrs,
            Item::Type(item) => &item.attrs,
            Item::Union(item) => &item.attrs,
            Item::Use(item) => &item.attrs,
            _ => &[],
        };
        if !test_only(attributes) {
            visit::visit_item(self, item);
        }
    }

    fn visit_signature(&mut self, signature: &'ast syn::Signature) {
        if let Some(token) = signature.asyncness {
            self.report(token.span, "async functions belong outside provider codecs");
        }
        visit::visit_signature(self, signature);
    }

    fn visit_expr_async(&mut self, expression: &'ast syn::ExprAsync) {
        self.report(
            expression.async_token.span,
            "async blocks belong outside provider codecs",
        );
        visit::visit_expr_async(self, expression);
    }

    fn visit_expr_await(&mut self, expression: &'ast syn::ExprAwait) {
        self.report(
            expression.await_token.span,
            "await belongs outside provider codecs",
        );
        visit::visit_expr_await(self, expression);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let names = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        if let Some(segment) = path.segments.first() {
            self.path(&names, segment.ident.span());
        }
        visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.imports(Vec::new(), &item.tree);
        visit::visit_item_use(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if let Some((_, path, _)) = &item.trait_
            && let Some(segment) = path.segments.last()
            && matches!(
                segment.ident.to_string().as_str(),
                "Transport" | "HttpClientExt"
            )
        {
            self.report(
                segment.ident.span(),
                "provider codecs cannot implement a transport",
            );
        }
        visit::visit_item_impl(self, item);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.macro_tokens(invocation.tokens.clone());
        visit::visit_macro(self, invocation);
    }
}

#[cfg(test)]
mod tests {
    use super::inspect;

    #[test]
    fn missing_empty_and_invalid_source_roots_fail() -> Result<(), crate::Error> {
        let root = tempfile::tempdir()?;
        if super::check(root.path()).is_ok() {
            return Err(crate::Error::Policy("missing source root passed".into()));
        }
        let directory = root.path().join("crates/ai/src/codecs/providers");
        std::fs::create_dir_all(&directory)?;
        if super::check(root.path()).is_ok() {
            return Err(crate::Error::Policy("empty source root passed".into()));
        }
        let source = directory.join("provider.rs");
        std::fs::write(&source, "fn encode(")?;
        if super::check(root.path()).is_ok() {
            return Err(crate::Error::Policy("unparseable source passed".into()));
        }
        std::fs::write(&source, "fn encode() {}")?;
        super::check(root.path())
    }

    fn rejected(source: &str) -> bool {
        matches!(inspect(source), Ok(findings) if !findings.is_empty())
    }

    #[test]
    fn rejects_effects_in_all_platform_branches_and_macro_awaits() {
        for source in [
            "async fn encode() {}",
            "fn encode() { let future = async {}; }",
            "fn encode() { request.await; }",
            "fn encode() { stream! { request.await; } }",
            "fn encode() { stream! { async {} } }",
            "#[cfg(target_family = \"wasm\")] async fn encode() {}",
            "fn encode() { std::env::var(\"API_KEY\"); }",
            "use std::{env::var as credential};",
            "use std::{fs as disk};",
            "use tokio::net::TcpStream;",
            "impl Transport for Provider {}",
            "impl HttpClientExt for Provider {}",
        ] {
            assert!(rejected(source), "accepted: {source}");
        }
        assert!(inspect("fn encode(").is_err());
    }

    #[test]
    fn permits_inline_tests_compile_time_constants_and_owned_data() {
        let source = r#"
            use std::{env::consts::OS, sync::Arc};
            struct Provider { model: Arc<str> }
            fn encode() { let text = "async request.await std::env::var"; }
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            // async fn commented() { network.await; }
            #[cfg(test)]
            mod tests {
                async fn fixture() { fake_socket.await; }
                fn macro_fixture() { stream! { fake_socket.await; } }
            }
            #[cfg(test)]
            async fn standalone_fixture() { fake_socket.await; }
        "#;
        assert!(matches!(inspect(source), Ok(findings) if findings.is_empty()));
    }
}
