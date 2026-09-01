# Changelog

## 0.3.0

A tab can be edited in place: a `Tab` that is `editing` is drawn as whatever
`PaneView::tab_editor_ui` puts in it — a box its title is retyped in — instead of its title,
and a double-clicked tab reports `FramesEvent::TabDoubleClicked`, which is how an application
knows to open one.

## 0.1.0

First release. Tabs, nested splits, dragged and dropped panes, and an arrangement that stores
and restores.
