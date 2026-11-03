# Cove Plugins

Plugins are plain JavaScript ES modules without a build step. Each plugin lives in
its own folder with a `manifest.json` and an entry module.

```
plugins/<id>/
  manifest.json   # id, name, version, entry, apiVersion
  index.js        # export function activate(ctx) { ... }
```

The plugin API (`ctx`) is not defined yet; `hello-world` shows the intended shape.
