blps = [
  'media_properties.blp',
  'page.blp',
  'page_grid.blp',
  'shortcuts.blp',
  'window.blp',
]

uis = []
foreach blp : blps
  uis += blp.replace('.blp', '.ui')
endforeach

blueprints = custom_target('blueprints',
  input: blps,
  output: uis,
  command: [find_program('blueprint-compiler'), 'batch-compile', '@OUTDIR@', '@CURRENT_SOURCE_DIR@', '@INPUT@'],
)
