"""Finder layout for the drag-to-Applications installer.

Only this trusted settings file is executed; app bundles are copied as data.
"""
from pathlib import Path

app = Path(defines["app"])
format = "UDZO"
filesystem = "HFS+"
files = [(str(app), "ac-man.app")]
symlinks = {"Applications": "/Applications"}
# Do not set FinderInfo on the signed bundle (including extension hiding):
# codesign rejects that metadata on the installed copy.
hide_extensions = []
background = defines["background"]
app_icon = app / "Contents/Resources/AppIcon.icns"
if app_icon.is_file():
    icon = str(app_icon)
window_rect = ((160, 160), (640, 300))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
include_icon_view_settings = True
include_list_view_settings = False
icon_size = 128
text_size = 16
label_pos = "bottom"
icon_locations = {"ac-man.app": (150, 140), "Applications": (490, 140)}
