# Summarize HTML error pages instead of printing them

Status: open
Related: 0006 (find public GraphQL servers), 0010 (clear errors)

## Problem

When a server answers an error with an HTML page, Vega prints the start of the page as the error message. Both of these were found while checking servers for 0006:

```
$ vega --uri https://bahnql.herokuapp.com/graphql --search ""
Error: fetching the schema from https://bahnql.herokuapp.com/graphql

Caused by:
    server answered HTTP 404: <!DOCTYPE html>
    <html>
      <head>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <meta charset="utf-8">
        <title>No such app</title>
        <style media="screen">
```

`https://mocki.io/graphql` is worse: the body starts with a `<head>` and an analytics `<script>`. The text is cut mid-tag and is useless. In the TUI the status bar shows only the first line, which for the first server is `server answered HTTP 404: <!DOCTYPE html>`.

## Expected behavior

When the body looks like HTML, the message says so and shows the `<title>` if there is one, for example `server answered HTTP 404: an HTML page titled "No such app"`. JSON and plain-text bodies are printed as today. A header value must never appear in the message, as today.
