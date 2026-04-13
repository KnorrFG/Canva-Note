# Canva Note

A native, infinite canvas note taking tool, working offline. It can hold markdown text and images.
It's not ready for use yet. For example it hardcodes any config (with my preferences) and it's not
feature complete yet.

## Features:
- Create, delete, drag and resize Nodes
- Supported Node types:
  - Markdown
  - Image
- Paste node from clipboard (with ctrl-p)
- Edit markdown nodes through an editor (by double clicking. Requires wezterm and helix, globally available as hx)
- Create a new text node by double clicking the background.
- Drag by holding right click.
- Drag the pane:
  - with right-click,
  - control-right-click while the mouse is over another node
  - hjkl keys.
- Select nodes with left click
- delete selected nodes with x, d, or del
- resize selected node with mouse
- undo (u, ctr-z)/redo (u, ctrl-y)
- Save (ctrl-s, the path must be passed as cli arg during startup)
- Zoom (+/- or ctrl + mouse wheel)


## Todo
- Proper Error handling
- embed arbitrary Files
- meta data
- launcher functionality
- index/meta-data search
- graph drawing functionality
- drag and drop
- camera location hotkeys
- actual config

## AI Disclaimer

Yes, I'm using AI. But I review and fix a lot of produced code (except for the
unit tests, tbh). I reject the term vibe-coded slop for my projects. This code
may not look exactly as if I had written it by hand, but it's close, and it's
also not worse.



