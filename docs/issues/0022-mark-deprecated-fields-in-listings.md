# Mark deprecated fields where fields are listed

Status: done
Related: 0006 (find public GraphQL servers)

## Problem

Vega fetches `isDeprecated` and `deprecationReason`, but shows them only when a single field is described (`--describe Type.field`, which ends with `Deprecated: <reason>`). Where fields are listed there is no marker:

```
$ vega --uri https://gitlab.com/api/graphql --describe AbuseReportUploadRegistry | grep forceToRedownload
  forceToRedownload: Boolean

$ vega --uri https://gitlab.com/api/graphql --describe AbuseReportUploadRegistry.forceToRedownload
...
Deprecated: Removed from registry tables in the database in favor of the newer reusable framework. Deprecated in GitLab 17.10.
```

The type listing of `--describe` and the field rows of the TUI Details pane (`tui/ui.rs` has no deprecation handling) look the same for a deprecated field as for a current one. GitLab's schema has 2189 deprecated fields, so a user building a query cannot tell which ones to avoid without opening each field.

Not checked: enum values (GitLab has 237 deprecated ones), which `describe` does not appear to mark either.

## Expected behavior

A deprecated field is marked in the `--describe` type listing and in the TUI field rows, with the reason available as it is today. Decide the marker (text such as `[deprecated]`, or dim style) with the colors and themes work in 0011.

## Decision

Deprecated fields get the text ` [deprecated]` after their signature in the `--describe` type listing and in the TUI tree rows. Text works in any terminal and in `--json`; a dim style can be added with the themes in 0011. Enum values are not marked yet.
