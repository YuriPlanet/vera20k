# dmgbuild settings for the VERA20k disk image (app.sh):
#   dmgbuild -s dmg_settings.py -D app=<VERA20k.app> VERA20k <image.dmg>
# The window shows dmgbuild's built-in arrow, a 640 x 240 point image, pointing
# from the app to the Applications folder it should be dragged onto.
app = defines["app"]  # noqa: F821 -- dmgbuild runs this file with `defines` set

files = [app]
symlinks = {"Applications": "/Applications"}
format = "UDZO"
# hdiutil compresses at zlib level 1, its fastest, unless told otherwise.
compression_level = 9

background = "builtin-arrow"
window_rect = ((200, 200), (640, 240))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 128
text_size = 13
icon_locations = {"VERA20k.app": (140, 120), "Applications": (500, 120)}
