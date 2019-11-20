# Rust Analyzer

Rust Analyzer is an **experimental** modular compiler frontend for the Rust
language. It is a part of a larger rls-2.0 effort to create excellent IDE
support for Rust. If you want to get involved, check the rls-2.0 working group
in the compiler-team repository:

https://github.com/rust-lang/compiler-team/tree/master/content/working-groups/rls-2.0

Work on the Rust Analyzer is sponsored by

[<img src="https://user-images.githubusercontent.com/1711539/58105231-cf306900-7bee-11e9-83d8-9f1102e59d29.png" alt="Ferrous Systems" width="300">](https://ferrous-systems.com/)
- [Mozilla](https://www.mozilla.org/en-US/)

## Language Server Quick Start

Rust Analyzer is a work-in-progress, so you'll have to build it from source, and
you might encounter critical bugs. That said, it is complete enough to provide a
useful IDE experience and some people use it as a daily driver.

To build rust-analyzer, you need:

* latest stable rust for language server itself
* latest stable npm and VS Code for VS Code extension

To quickly install rust-analyzer with VS Code extension with standard setup
(`code` and `cargo` in `$PATH`, etc), use this:

```
# clone the repo
$ git clone https://github.com/rust-analyzer/rust-analyzer && cd rust-analyzer

# install both the language server and VS Code extension
$ cargo xtask install

# alternatively, install only the server. Binary name is `ra_lsp_server`.
$ cargo xtask install --server
```

For non-standard setup of VS Code and other editors, see [./docs/user](./docs/user).

## Documentation

If you want to **contribute** to rust-analyzer or just curious about how things work
under the hood, check the [./docs/dev](./docs/dev) folder.

If you want to **use** rust-analyzer's language server with your editor of
choice, check [./docs/user](./docs/user) folder. It also contains some tips & tricks to help
you be more productive when using rust-analyzer.

## Getting in touch

We are on the rust-lang Zulip!

https://rust-lang.zulipchat.com/#narrow/stream/185405-t-compiler.2Frls-2.2E0

## Quick Links

* API docs: https://rust-analyzer.github.io/rust-analyzer/ra_ide_api/

## License

Rust analyzer is primarily distributed under the terms of both the MIT
license and the Apache License (Version 2.0).

See LICENSE-APACHE and LICENSE-MIT for details.
