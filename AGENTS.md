# omni

omni is a universal formatter that always picks the right formatter for the right file based on your
project and configuration.  no more per-editor per-project bullshit to decide between oxfmt, rsfmt,
prettier, etc.  it just always does the right thing

it's just for editors, not a CLI.  so it only supports formatting a single file or stdin

## Rules

- always write doc comments for module exports
