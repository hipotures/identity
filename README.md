# Identity (fork)

> **This is a personal fork of [YaLTeR/identity](https://gitlab.gnome.org/YaLTeR/identity) with additional features.**
>
> **Changes added in this fork:**
> - Image rotation button (90°/180°/270°) with toast notification
> - Playback speed dropdown control (1×, 0.5×, 0.1×, 1 FPS)

---

A program for comparing multiple versions of an image or video.

<a href='https://flathub.org/apps/details/org.gnome.gitlab.YaLTeR.Identity'><img alt='Download on Flathub' src='https://flathub.org/api/badge?svg&locale=en'/></a>

![Screenshot of the window.](https://gitlab.gnome.org/-/project/12785/uploads/2c4c198f8837f57e9f2f96a69538631c/image.png)

## Running

You can run Identity as is and select files to compare using the Open button. You can also pass file paths or URIs as command-line arguments:

```
$ identity path/to/file1.mp4 path/to/file2.mp4
```

Note that Flatpak Identity doesn't have access to the filesystem, so files need to be forwarded manually like so:

```
$ flatpak run --file-forwarding org.gnome.gitlab.YaLTeR.Identity @@ path/to/file1.mp4 path/to/file2.mp4
```

Use `@@u` instead of `@@` to pass URIs.

## Format support

Identity uses GStreamer, and therefore your system's or Flatpak GNOME Platform's installed GStreamer plugins. In particular, Identity won't work at all without the `playbin3` element (typically in `gst-plugins-base`).

For showing images, Identity uses [glycin]. When packaging Identity, remember to add runtime dependencies needed by glycin, like glycin-loaders. You can find the full list in [glycin's documentation](https://docs.rs/glycin/latest/glycin/#external-dependencies).

## Contributing translations

You can help translate Identity: https://l10n.gnome.org/module/identity/. Any help is appreciated!

## Building

The easiest way is to clone the repository with GNOME Builder and press the Build button.

Alternatively, you can build it manually:
```
meson -Dprofile=development -Dprefix=$PWD/install build
ninja -C build install
```

## Code of Conduct

When interacting with the project, the [GNOME Code of Conduct](https://conduct.gnome.org) applies.

[glycin]: https://gitlab.gnome.org/sophie-h/glycin
