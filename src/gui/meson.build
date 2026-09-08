# Meson build file for TripleWrapper GUI

project('triplewrapper-gui', 'python',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
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
install_data('data/icons/triplewrapper.svg', install_dir: icon_dir, rename: 'com.triplewrapper.TripleWrapper.svg')
install_data('data/applications/com.triplewrapper.TripleWrapper.desktop', install_dir: desktop_dir)
install_data('data/metainfo/com.triplewrapper.TripleWrapper.metainfo.xml', install_dir: metainfo_dir)
install_data('data/glib-2.0/schemas/com.triplewrapper.TripleWrapper.gschema.xml', install_dir: schemas_dir)

# Python package
python.install_sources(
  ['triplewrapper_gui/__init__.py',
   'triplewrapper_gui/main.py',
   'triplewrapper_gui/window.py',
   'triplewrapper_gui/widgets/__init__.py',
   'triplewrapper_gui/widgets/graph.py',
   'triplewrapper_gui/widgets/donut.py',
   'triplewrapper_gui/widgets/disk_panel.py',
   'triplewrapper_gui/core/__init__.py',
   'triplewrapper_gui/core/client.py',
   'triplewrapper_gui/core/engine.py',
   'triplewrapper_gui/utils/__init__.py',
   'triplewrapper_gui/utils/formatting.py',
   'triplewrapper_gui/utils/settings.py'],
  install_dir: python.get_install_dir() / 'triplewrapper_gui'
)

# Executable script
exe = python.install_script('triplewrapper-gui', install_dir: get_option('bindir'))

# Tests
test('gui-tests', python.find_installation().python,
  args: ['-m', 'pytest', 'tests/'],
  dependencies: [exe],
  is_parallel: false
)

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