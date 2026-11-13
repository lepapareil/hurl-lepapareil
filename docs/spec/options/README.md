# Command Line Options

Hurl command line options are described with `.option` files. These files are used to generate:

- Rust code to parse this options in `hurl`/`hurlfmt` packages
- Man options
- Shell completions
- The catalog of options of Hurl UI (`hurlui_core` package)


## Format all option files

```shell
$ bin/spec/options/format.py docs/spec/options/**/*.option
```

This script insures that all `.option` files are well formatted.

## Generate/Update clap command source file

```shell
$ bin/spec/options/generate_source.py docs/spec/options/hurl/*.option > packages/hurl/src/cli/options/commands.rs
$ bin/spec/options/generate_source.py docs/spec/options/hurlfmt/*.option >packages/hurlfmt/src/cli/options/commands.rs
```

This script generates Rust source file for parsing command line option.

## Generate/Update man options

```shell
$ bin/spec/options/generate_man.py docs/spec/options/hurl/*.option
$ bin/spec/options/generate_man.py docs/spec/options/hurlfmt/*.option
```

This script generates (part of) man file for `hurl` / `hurlfmt`.

## Generate/Update Hurl UI options catalog

```shell
$ bin/spec/options/generate_ui.py docs/spec/options/hurl/*.option > packages/hurlui_core/src/options/hurl.json
```

This script generates the catalog of `hurl` options embedded in Hurl UI: its preferences, the suggestions of its
`[Options]` editor and the `hurl` command line it runs are built from this catalog, so that Hurl UI needs no change
when an option is added to Hurl. The options of an `[Options]` section and the type of their value are read from
`packages/hurl_core/src/ast/option.rs` (duration units from `packages/hurl_core/src/types.rs`); the script fails if an
option file and this file disagree on `cli_only`, or if an option has a type of value the script does not know.

A script is provided to run all these commands:

```shell
$ bin/spec/options/generate_all.py
```
