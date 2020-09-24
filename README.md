# Identity

A program for comparing multiple versions of an image or video.

![Screenshot of the window.](data/resources/screenshots/screenshot1.png)

## Building

The easiest way is to clone the repository with GNOME Builder and press the Build button.

Alternatively, you can build it manually:
```
meson -Dprofile=development -Dprefix=$PWD/install build
ninja -C build install
```
