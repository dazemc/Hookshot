// Builder configuration and environment: builder.ini, tool paths, constants.
// Mirrors builder/linux/hookshot_builder.py's load_config/save_config, ROOT
// resolution, and nx2/make_game.py's LANGUAGES.
import 'dart:io';

import 'package:path/path.dart' as p;

const appId = 'io.github.NotRexed.Hookshot';
const builderVersion = '1.0.0'; // (the desktop app's own version; the tools/game version is separate)
const defaultRepository = 'dazemc/Hookshot-rs';
const tracks = ['release', 'rc', 'nightly'];
const trackNames = {'release': 'Release', 'rc': 'Release candidate', 'nightly': 'Nightly'};

/// (code, human name), as nx2/make_game.py LANGUAGES.
const languages = [
  ('en-US', 'English (US)'),
  ('en-GB', 'English (UK)'),
  ('fr', 'French (France)'),
  ('fr-CA', 'French (Canada)'),
  ('de', 'German'),
  ('it', 'Italian'),
  ('es', 'Spanish (Spain)'),
  ('es-419', 'Spanish (Latin America)'),
  ('pt-BR', 'Portuguese (Brazil)'),
  ('nl', 'Dutch'),
  ('pl', 'Polish'),
  ('ru', 'Russian'),
  ('ja', 'Japanese'),
  ('ko', 'Korean'),
  ('zh-Hans', 'Chinese (Simplified)'),
  ('zh-Hant', 'Chinese (Traditional)'),
];

const dlssLicense = '''
NVIDIA DLSS is NVIDIA's, not Hookshot's. With it ticked, the build downloads NVIDIA's DLSS SDK (release v310.9.16) from github.com/NVIDIA/DLSS, under NVIDIA's own license, the NVIDIA RTX SDKs license:

https://github.com/NVIDIA/DLSS/blob/9981c6350dc58c58e75131689e19aa547db65571/LICENSE.txt

Its files stay on this PC (in the builder's ngx-sdk folder, and nvngx_dlss.dll and nvngx_dlssg.dll, or libnvidia-ngx-dlss.so and libnvidia-ngx-dlssg.so on Linux, in the game folder's ngx folder). Hookshot never includes or shares them: never share a game folder built with them.

Do you accept NVIDIA's license?''';

String get _home => Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'] ?? '~';

/// The marker a portable Builder home carries, written by the Windows setup
/// (`builder/setup/portable_home.h`) and by a hand-made portable folder.
const portableMarker = '.hookshot-portable';
const portableMarkerText = 'Hookshot Builder portable home v1';

/// The game's own, relative link to its Builder (the game launcher,
/// builder/setup/portable_home.h, and the packager read the same file).
const gameHomeMarker = '.hookshot_builder_home';

/// A resolved Builder home. [portable] is true only for a marked folder.
class BuilderHome {
  final String root;
  final bool portable;
  const BuilderHome(this.root, this.portable);
}

bool _markerValid(String root, p.Context ctx) {
  try {
    final bytes = File(ctx.join(root, portableMarker)).readAsBytesSync();
    if (bytes.length > portableMarkerText.length + 2) return false;
    var end = bytes.length;
    while (end > 0 && (bytes[end - 1] == 13 || bytes[end - 1] == 10)) {
      end--;
    }
    final text = String.fromCharCodes(bytes.sublist(0, end));
    return text == portableMarkerText && ctx.dirname(root) != root;
  } catch (_) {
    return false;
  }
}

bool _markerExists(String root, p.Context ctx) => File(ctx.join(root, portableMarker)).existsSync();

String _absolute(String path, p.Context ctx) {
  if (!ctx.isAbsolute(path)) {
    throw StateError('The Builder home must be a fully qualified folder: $path');
  }
  return ctx.normalize(path);
}

String _standardHome(Map<String, String> env, bool windows, p.Context ctx) {
  if (windows) {
    final local = env['LOCALAPPDATA'];
    final home = env['USERPROFILE'] ?? env['HOME'] ?? '~';
    final root = local != null && local.isNotEmpty ? local : ctx.join(home, 'AppData', 'Local');
    return ctx.join(root, 'HookshotBuilder');
  }
  final data = env['XDG_DATA_HOME'];
  final home = env['HOME'] ?? env['USERPROFILE'] ?? '~';
  final base = data != null && data.isNotEmpty ? data : ctx.join(home, '.local', 'share');
  return ctx.join(base, 'hookshot', 'builder');
}

/// Find a marked home from the running app: the setup installs it at
/// `versions/<tag>/HookshotBuilder`, a ZIP at `builders/<tag>/HookshotBuilder`,
/// and both sit under the marked home. A broken marker keeps that folder as
/// the home (never AppData); only a valid marker makes it portable.
BuilderHome? _discoverFromExecutable(String executable, p.Context ctx) {
  var directory = ctx.dirname(_absolute(executable, ctx));
  final executableDirectory = directory;
  for (var depth = 0; depth <= 3; depth++) {
    if (_markerValid(directory, ctx)) return BuilderHome(directory, true);
    if (_markerExists(directory, ctx)) return BuilderHome(directory, false);
    final parent = ctx.dirname(directory);
    if (parent == directory) break;
    directory = parent;
  }
  final sibling = ctx.join(executableDirectory, 'HookshotBuilder');
  if (_markerValid(sibling, ctx)) return BuilderHome(sibling, true);
  if (_markerExists(sibling, ctx)) return BuilderHome(sibling, false);
  return null;
}

/// The Builder's data home: a marked portable home, the setup's explicit
/// `HOOKSHOT_SETUP_BASE`, the old `portable.txt` layout, or the standard
/// per-user folder. [standard] and [executable] let tests use authored paths.
BuilderHome resolveBuilderHome({
  Map<String, String>? environment,
  String? executable,
  bool? windows,
  String? standard,
}) {
  final env = environment ?? Platform.environment;
  final win = windows ?? Platform.isWindows;
  final ctx = win ? p.windows : p.posix;
  final portableRequested = env['HOOKSHOT_BUILDER_PORTABLE'] == '1';
  final requested = (env['HOOKSHOT_SETUP_BASE'] ?? '').trim();
  if (requested.isNotEmpty) {
    final root = _absolute(requested, ctx);
    final marked = _markerValid(root, ctx);
    if (portableRequested && !marked) {
      throw StateError('The portable Builder home is missing its marker: $root');
    }
    return BuilderHome(root, marked);
  }
  final found = _discoverFromExecutable(executable ?? Platform.resolvedExecutable, ctx);
  if (portableRequested) {
    if (found != null && found.portable) return found;
    throw StateError('The portable Builder home was not passed to this process.');
  }
  if (found != null) return found;
  // (the earlier opt-in: an empty portable.txt beside the executable keeps
  // settings, downloads and backups under <exe>/data)
  final exeDir = ctx.dirname(_absolute(executable ?? Platform.resolvedExecutable, ctx));
  if (File(ctx.join(exeDir, 'portable.txt')).existsSync()) {
    final data = ctx.join(exeDir, 'data');
    return BuilderHome(
      win ? ctx.join(data, 'HookshotBuilder') : ctx.join(data, 'hookshot', 'builder'),
      true,
    );
  }
  final override = env['HOOKSHOT_BUILDER_HOME'];
  final root = standard ??
      (override != null && override.isNotEmpty
          ? (win ? ctx.join(override, 'HookshotBuilder') : ctx.join(override, 'hookshot', 'builder'))
          : _standardHome(env, win, ctx));
  return BuilderHome(_absolute(root, ctx), false);
}

/// Per-user Hookshot Builder data, or the portable home beside the setup.
String builderHome({
  Map<String, String>? environment,
  String? executable,
  bool? windows,
  String? standard,
}) =>
    resolveBuilderHome(
      environment: environment,
      executable: executable,
      windows: windows,
      standard: standard,
    ).root;

bool isPortable({
  Map<String, String>? environment,
  String? executable,
  bool? windows,
  String? standard,
}) =>
    resolveBuilderHome(
      environment: environment,
      executable: executable,
      windows: windows,
      standard: standard,
    ).portable;

/// The default game folder: `Hookshot` beside a portable home, or the usual
/// `~/Games/Hookshot`.
String defaultGameFolder({
  Map<String, String>? environment,
  String? executable,
  bool? windows,
  String? standard,
}) {
  final env = environment ?? Platform.environment;
  final win = windows ?? Platform.isWindows;
  final ctx = win ? p.windows : p.posix;
  final home = resolveBuilderHome(
    environment: env,
    executable: executable,
    windows: win,
    standard: standard,
  );
  if (home.portable) return ctx.join(ctx.dirname(home.root), 'Hookshot');
  final user = env['HOME'] ?? env['USERPROFILE'] ?? '~';
  return ctx.join(user, 'Games', 'Hookshot');
}

String configPath({
  Map<String, String>? environment,
  String? executable,
  bool? windows,
  String? standard,
}) {
  final env = environment ?? Platform.environment;
  final win = windows ?? Platform.isWindows;
  final ctx = win ? p.windows : p.posix;
  final home = resolveBuilderHome(
    environment: env,
    executable: executable,
    windows: win,
    standard: standard,
  );
  if (home.portable || win) return ctx.join(home.root, 'builder.ini');
  // (the same file the Tk builder and hookshot-launch read on Linux)
  final xdg = env['XDG_CONFIG_HOME'];
  final homeFolder = env['HOME'] ?? env['USERPROFILE'] ?? '~';
  final base = xdg != null && xdg.isNotEmpty ? xdg : ctx.join(homeFolder, '.config');
  return ctx.join(base, 'hookshot', 'builder.ini');
}

/// The tools root: HOOKSHOT_TOOLS when packaged, else this repository checkout.
String toolsRoot() {
  final tools = Platform.environment['HOOKSHOT_TOOLS'];
  if (tools != null && tools.isNotEmpty) return tools;
  // Walk up from the running binary until the sources are found: a packaged
  // builder keeps them in <app>/tools, a source checkout at the repository
  // root (debug bundle, test runner and packaged layouts all differ).
  bool hasSources(String path) =>
      Directory('$path/third_party/a64-kit').existsSync() || File('$path/builder/rust/Cargo.toml').existsSync();
  try {
    var dir = File(Platform.resolvedExecutable).parent;
    for (var i = 0; i < 10; i++) {
      if (hasSources('${dir.path}/tools')) return '${dir.path}/tools';
      if (hasSources(dir.path)) return dir.path;
      final parent = dir.parent;
      if (parent.path == dir.path) break;
      dir = parent;
    }
  } catch (_) {}
  var dir = Directory(File(Platform.script.toFilePath()).parent.path);
  for (var i = 0; i < 10; i++) {
    if (hasSources('${dir.path}/tools')) return '${dir.path}/tools';
    if (hasSources(dir.path)) return dir.path;
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  return Directory.current.path;
}

final _repositoryPattern = RegExp(r'^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$');

/// Repository releases come from. Packaged releases carry repository.txt;
/// source builds can override it with HOOKSHOT_REPOSITORY.
String repository({String? override, String? root}) {
  final env = override ?? Platform.environment['HOOKSHOT_REPOSITORY'];
  if (env != null && _repositoryPattern.hasMatch(env.trim())) return env.trim();
  final base = root ?? toolsRoot();
  for (final path in ['$base/repository.txt', '$base/tools/repository.txt']) {
    try {
      final candidate = File(path).readAsLinesSync().first.trim();
      if (_repositoryPattern.hasMatch(candidate)) return candidate;
    } catch (_) {}
  }
  return defaultRepository;
}

/// Path values builder.ini keeps; in a portable home they are stored relative
/// to the home so a moved folder still finds its game and its files.
const configPathKeys = {'out', 'nup', 'romfs', 'exefs'};

void _checkConfigPath(String value, p.Context ctx) {
  if (value.contains('\r') || value.contains('\n') || value.contains('\x00')) {
    throw const FormatException('A Builder path cannot contain a line break.');
  }
  if (ctx.style == p.Style.windows && !ctx.isAbsolute(value) && value.contains(':')) {
    throw const FormatException('A portable Builder path cannot name another drive.');
  }
}

/// Store a same-volume path relative to [home]; another volume stays absolute.
String portableSavePath(String path, String home, {bool? windows}) {
  final ctx = (windows ?? Platform.isWindows) ? p.windows : p.posix;
  if (path.isEmpty) return path;
  _checkConfigPath(path, ctx);
  if (!ctx.isAbsolute(path) || !ctx.isAbsolute(home)) return path;
  if (!ctx.equals(ctx.rootPrefix(path), ctx.rootPrefix(home))) return path;
  final relative = ctx.relative(path, from: home);
  _checkConfigPath(relative, ctx);
  return relative;
}

/// Resolve a stored path against the current home; an absolute path is kept.
String portableLoadPath(String path, String home, {bool? windows}) {
  final ctx = (windows ?? Platform.isWindows) ? p.windows : p.posix;
  if (path.isEmpty) return path;
  _checkConfigPath(path, ctx);
  if (ctx.isAbsolute(path)) return path;
  return ctx.normalize(ctx.join(home, path));
}

Map<String, String> loadConfig({
  String? path,
  String? home,
  bool? portable,
  bool? windows,
}) {
  final win = windows ?? Platform.isWindows;
  final ctx = win ? p.windows : p.posix;
  final root = home ?? builderHome(windows: win);
  final portableHome = portable ?? (home != null ? _markerValid(root, ctx) : isPortable(windows: win));
  final cfg = <String, String>{};
  try {
    for (final raw in File(path ?? configPath(windows: win)).readAsLinesSync()) {
      final line = raw.trim();
      if (line.isEmpty) continue;
      final i = line.indexOf('=');
      if (i <= 0) continue;
      final key = line.substring(0, i);
      var value = line.substring(i + 1);
      if (portableHome && configPathKeys.contains(key)) {
        try {
          value = portableLoadPath(value, root, windows: win);
        } catch (_) {}
      }
      cfg[key] = value;
    }
  } catch (_) {}
  return cfg;
}

void saveConfig(
  Map<String, String> cfg, {
  String? path,
  String? home,
  bool? portable,
  bool? windows,
}) {
  final win = windows ?? Platform.isWindows;
  final ctx = win ? p.windows : p.posix;
  final root = home ?? builderHome(windows: win);
  final portableHome = portable ?? (home != null ? _markerValid(root, ctx) : isPortable(windows: win));
  final stored = <String, String>{};
  for (final entry in cfg.entries) {
    var value = entry.value;
    if (portableHome && configPathKeys.contains(entry.key) && value.isNotEmpty) {
      try {
        value = portableSavePath(value, root, windows: win);
      } catch (_) {}
    }
    stored[entry.key] = value;
  }
  final target = path ?? configPath(windows: win);
  Directory(ctx.dirname(target)).createSync(recursive: true);
  File(target).writeAsStringSync(stored.entries.map((e) => '${e.key}=${e.value}\n').join());
}

/// Free GB under [folder] (nearest existing ancestor), null when unknown.
double? freeGb(String folder) {
  try {
    if (Platform.isWindows) {
      const script =
          r'''$path=$env:HOOKSHOT_FREE_PATH; while(-not [IO.Directory]::Exists($path)) { $parent=[IO.Directory]::GetParent($path); if($null -eq $parent) { break }; $path=$parent.FullName }; $root=[IO.Path]::GetPathRoot($path); [long]([IO.DriveInfo]::new($root).AvailableFreeSpace)''';
      final result = Process.runSync('powershell.exe', [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        script,
      ], environment: {
        ...Platform.environment,
        'HOOKSHOT_FREE_PATH': folder.isEmpty ? _home : folder
      });
      if (result.exitCode != 0) return null;
      return int.parse((result.stdout as String).trim()) / (1 << 30);
    }
    var path = folder.isEmpty ? '/' : folder;
    while (path != '/' && !Directory(path).existsSync()) {
      final parent = Directory(path).parent.path;
      if (parent == path) break;
      path = parent;
    }
    final result = Process.runSync('df', ['-k', '--output=avail', path]);
    if (result.exitCode != 0) return null;
    final lines = (result.stdout as String).trim().split('\n');
    if (lines.length < 2) return null;
    return int.parse(lines.last.trim()) / (1 << 20);
  } catch (_) {
    return null;
  }
}
