# waterui-svg

High-performance SVG rendering for WaterUI using Vello.

## Text, fonts and cargo features

Everything this crate draws is geometry: paths, images, and — when a document
has text and a font to set it in — the outlines `usvg` flattens that text into.
It never asks the operating system for a font, so a drawing looks the same on
every backend and on a device with no font catalogue at all.

| Feature | Default | Effect |
| --- | --- | --- |
| `text` | off | Enables `usvg/text`, which lays `<text>` out into outline paths, and the `SvgSceneContent::with_fonts` constructor that gives the parser a font database to lay it out with. Pulls in `fontdb`, `rustybuzz`, `unicode-bidi`, `unicode-script` and `unicode-vo`. |

`usvg` is depended on with `default-features = false`, so its own defaults are
named here rather than inherited:

| `usvg` feature | Enabled | Why |
| --- | --- | --- |
| `text` | through this crate's `text` feature | It is the only `usvg` feature that changes what this crate draws: with it off the parser drops `<text>` outright; with it on, and given fonts, the element becomes the flattened outline group the renderer already draws. |
| `system-fonts` | no | It widens `fontdb` towards the filesystem (font directory scanning, fontconfig). This crate never asks `fontdb` to read a file — it is handed a database or it parses with an empty one — so the feature cannot affect output. A text fixture naming a specific family renders identically with and without it, as does one asking for a generic family. |
| `memmap-fonts` | no | It memory-maps font files that `fontdb` loaded from disk. Nothing here loads a font from disk. |

An application that wants the operating system's fonts owns that decision
itself: it builds a `fontdb::Database` (with whichever of `fontdb`'s own
filesystem features it wants), fills it, and passes it to
`SvgSceneContent::with_fonts`. Without fonts — and that includes the `Svg` view,
which is the icon primitive and always parses without them — a `<text>` element
is dropped during parsing and draws nothing.

## Accessible name

A document that names itself with a root `<title>` reaches a screen reader
under that name:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
  <title>Warning sign</title>
  <path d="M12 2L22 20H2Z"/>
</svg>
```

The title is the name the drawing *offers*. An application's own
`.a11y_label(…)` on the view or any ancestor wins over it, because the
application knows what the icon is for. A `<title>` nested inside an element
names that element rather than the document and is not used; path data
(`Svg::from_path`, `Svg::from_stroke_path`) carries no title, so those icons
stay unnamed until the application names them.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
