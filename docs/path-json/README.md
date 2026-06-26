# Identity Path JSON

Identity can record a path of clicked points in the displayed image or video.
Press `x` to start recording, click points in the media, then press `x` again
to write a JSON file to `/tmp/identity-path-[timestamp].json`.

Holding `Shift` while clicking is ignored by the recorder. This leaves
Shift-assisted panning workflows available without creating unwanted points.

## Coordinate System

Coordinates use the standard raster image and FFmpeg convention:

- `(0, 0)` is the top-left source pixel.
- `x` increases to the right.
- `y` increases downward.
- `x` and `y` are integer source-pixel indices after zoom, scroll, and rotation
  have been mapped back to the original media dimensions.

For a 7680x4320 video, the bottom-right pixel is `(7679, 4319)`.

## Schema

```json
{
  "format": "identity-path-v1",
  "created_at_unix_ms": 1782470000000,
  "source": {
    "uri": "file:///path/to/moon8k.mov",
    "display_name": "moon8k.mov",
    "width": 7680,
    "height": 4320
  },
  "time_unit": "seconds",
  "coordinate_space": "source_pixels_top_left_origin",
  "points": [
    { "x": 2450, "y": 2100, "t": 12.345678 },
    { "x": 5200, "y": 1850, "t": 165.0 }
  ]
}
```

`t` is the current playback position in seconds. For still images, `t` is `0`.

If multiple files are open, the first recorded point pins the session to that
file. Later clicks on other files are ignored.

## FFmpeg Crop Usage

FFmpeg's `crop` filter also uses top-left image coordinates, but its `x` and
`y` values are the top-left corner of the crop rectangle, not the center.

If an Identity path point should be the center of a 1920x1080 crop:

```text
crop_x = point.x - 960
crop_y = point.y - 540
```

Clamp those values before calling FFmpeg so the crop rectangle stays inside the
source frame:

```text
crop_x = clamp(point.x - 960, 0, source.width - 1920)
crop_y = clamp(point.y - 540, 0, source.height - 1080)
```
