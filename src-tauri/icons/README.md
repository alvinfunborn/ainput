# AInput icons

`icon.png` is the shared A/caret artwork, corrected with the built-in imagegen
tool from the original app icon. `icon.ico` remains the existing Windows asset.

`bash scripts/generate-macos-icons.sh` uses Xcode Icon Composer to export the
native macOS silhouette. `icon-macos.png` and `icon.icns` include standard app
icon padding. The 32/128/256px PNGs and `tray-macos.png` use that same artwork
without the outer app-icon padding for settings and the menu bar.

Image edit prompt:

> Use case: precise-object-edit. Edit target: the supplied AInput app icon.
> Produce one clean square 1024x1024 production PNG icon with real transparent
> corners. Preserve the original blue-to-violet diagonal gradient, the bold
> white capital A and the slim white vertical typing caret to its right.
> Correct the right-heavy placement: move the combined A and caret mark left
> so the entire white mark is visually centered horizontally within the tile,
> and centered vertically. Keep the original relative A and caret proportions;
> white mark should occupy approximately half the tile width and half its
> height. Change the tile from a hard square to a smooth macOS-style rounded
> square with corner radius approximately 23% of tile width, filling the canvas
> edge to edge except the transparent rounded corners. Crisp flat vector-like
> edges, keep the gradient smooth. No glow on the caret, no shadows, no bevel,
> no border, no background outside the rounded square, no new symbols, no extra
> text. Preserve AInput identity, only correct centering and corners.
