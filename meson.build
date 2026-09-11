# Meson build file for TripleWrapper GUI

project('triplewrapper-gui',
  version: '0.1.0',
  license: 'MIT',
  default_options: ['warning_level=2', 'werror=false']
)

# Python dependencies
python = import('python')
pygobject_dep = dependency('pygobject-3.0', version: '>=3.48')
gi_dep = dependency('gi', version: '>=3.48')
cairo_dep = dependency('cairo', version: '>=1.17')
graphene_dep = dependency('graphene-1.0', version: '>=1.10')
gtk4_dep = dependency('gtk4', version: '>=4.10')
libadwaita_dep = dependency('libadwaita-1', version: '>=1.4')

# Check for required modules
pygobject = python.import('gi')
pygobject.check_version('3.48')

# Data files
data_dir = join_paths(get_option('datadir'), 'triplewrapper')
icon_dir = join_paths(data_dir, 'icons/hicolor/scalable/apps')
desktop_dir = join_paths(data_dir, 'applications')
metainfo_dir = join_paths(data_dir, 'metainfo')
schemas_dir = join_paths(get_option('datadir'), 'glib-2.0/schemas')

# Install data files
install_data('triplewrapper_gui/data/icons/triplewrapper.svg', install_dir: icon_dir, rename: 'com.github.triplewrapper.App.svg')
install_data('triplewrapper_gui/data/applications/com.github.triplewrapper.App.desktop', install_dir: desktop_dir)
install_data('triplewrapper_gui/data/metainfo/com.github.triplewrapper.App.metainfo.xml', install_dir: metainfo_dir)
install_data('triplewrapper_gui/data/glib-2.0/schemas/com.github.triplewrapper.App.gschema.xml', install_dir: schemas_dir)

# Python package
python.install_sources(
  ['triplewrapper_gui/__init__.py',
   'triplewrapper_gui/main.py',
   'triplewrapper_gui/application.py',
   'triplewrapper_gui/main_window.py',
   'triplewrapper_gui/views/__init__.py',
   'triplewrapper_gui/views/welcome_view.py',
   'triplewrapper_gui/views/analysis_view.py',
   'triplewrapper_gui/views/progress_view.py',
   'triplewrapper_gui/widgets/__init__.py',
   'triplewrapper_gui/widgets/steam_graph.py',
   'triplewrapper_gui/widgets/storage_donut.py',
   'triplewrapper_gui/widgets/queue_panel.py',
   'triplewrapper_gui/core/__init__.py',
   'triplewrapper_gui/core/bridge.py',
   'triplewrapper_gui/core/models.py',
   'triplewrapper_gui/core/protocol.py'],
  install_dir: python.get_install_dir() / 'triplewrapper_gui'
)

# Executable script
exe = python.install_script('triplewrapper-gui', install_dir: get_option('bindir'))

# Summary
summary({
  'Project': 'TripleWrapper GUI',
  'Version': meson.project_version(),
  'GTK4': gtk4_dep.version(),
  'Libadwaita': libadwaita_dep.version(),
  'Graphene': graphene_dep.version(),
  'Install prefix': get_option('prefix'),
  'Data dir': data_dir,
})