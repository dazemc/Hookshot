// The Rust engine's build paths: build.all for an unpacked folder or for a
// .nup's read result, and the JSON requests --stream reads.
import 'dart:convert';
import 'dart:io';

import 'package:path/path.dart' as p;

import '../config.dart' as cfg;
import '../engine.dart' show RustEngineMissing;

/// The engine CLI beside the app, in the checkout, or named by
/// HOOKSHOT_ENGINE_CLI. Null leaves the build to make_game.py.
String? rustEngineCli({String? override, String? root, String? exeDir}) {
  final configured = override ?? Platform.environment['HOOKSHOT_ENGINE_CLI'];
  final tools = root ?? cfg.toolsRoot();
  final exe = exeDir ?? File(Platform.resolvedExecutable).parent.path;
  final name = Platform.isWindows ? 'hookshot_engine.exe' : 'hookshot_engine';
  for (final path in [
    if (configured != null && configured.isNotEmpty) configured,
    p.join(exe, name),
    p.join(exe, 'lib', name),
    p.join(tools, 'builder', 'rust', 'target', 'release', name),
    p.join(tools, 'builder', 'rust', 'target', 'debug', name),
  ]) {
    if (File(path).existsSync()) return path;
  }
  return null;
}

/// The CLI a stage that has no Python path anymore must have. A NUP build
/// stops with this typed error instead of looking for a fallback.
String requireRustEngineCli({String? override, String? root, String? exeDir}) {
  final cli = rustEngineCli(override: override, root: root, exeDir: exeDir);
  if (cli == null) throw const RustEngineMissing();
  return cli;
}

/// NUP builds are read by the engine; folder builds keep their own route.
bool rustNupApplies({required String kind}) => kind == 'nup';

/// A DLSS build the Rust engine cannot make yet. The Windows Vulkan DLL's
/// NGX link is still Python's, and this builder no longer falls back to it,
/// so the build stops with this typed error instead.
class DlssBuildUnsupported implements Exception {
  final String message;
  const DlssBuildUnsupported([
    this.message = 'NVIDIA DLSS builds on Windows are not in the Rust engine yet: the Windows Vulkan '
        'backend link is still Python\'s, and the builder no longer falls back to it. Build '
        'without NVIDIA DLSS, or build with the Python path from a source checkout.',
  ]);
  @override
  String toString() => message;
}

/// Throws [DlssBuildUnsupported] for a DLSS build on Windows. `windows`
/// overrides the platform in tests.
void requireDlssSupported({required bool dlss, bool? windows}) {
  if (dlss && (windows ?? Platform.isWindows)) {
    throw const DlssBuildUnsupported();
  }
}

/// Where the builder keeps NVIDIA's DLSS SDK between builds: the same folder
/// the Python path used (`%LOCALAPPDATA%\HookshotBuilder\ngx-sdk` on Windows,
/// `$XDG_DATA_HOME/hookshot/builder/ngx-sdk` on Linux).
String dlssSdkDir({
  bool? windows,
  String? localAppData,
  String? xdgDataHome,
  String? home,
}) {
  String read(String name) => Platform.environment[name] ?? '';
  if (windows ?? Platform.isWindows) {
    final local = localAppData ?? read('LOCALAPPDATA');
    return p.join(
      local.isNotEmpty ? local : cfg.toolsRoot(),
      'HookshotBuilder',
      'ngx-sdk',
    );
  }
  final data = xdgDataHome ?? read('XDG_DATA_HOME');
  final userHome = home ?? (read('HOME').isNotEmpty ? read('HOME') : '~');
  final root = data.isNotEmpty ? data : p.join(userHome, '.local', 'share');
  return p.join(root, 'hookshot', 'builder', 'ngx-sdk');
}

/// make_game.py's --work default: build files kept apart from the game folder.
String rustBuildWork() {
  if (Platform.isWindows) {
    final local = Platform.environment['LOCALAPPDATA'];
    return p.join(
      local != null && local.isNotEmpty ? local : cfg.toolsRoot(),
      'HookshotBuilder',
    );
  }
  final cache = Platform.environment['XDG_CACHE_HOME'];
  final home = Platform.environment['HOME'] ?? '~';
  return p.join(
    cache != null && cache.isNotEmpty ? cache : p.join(home, '.cache'),
    'hookshot-builder',
  );
}

/// build.all needs an unpacked exefs/romfs pair. Shader preparation and the
/// NVIDIA DLSS stage are Rust's now, so only the engine's presence decides.
bool rustBuildApplies({required String kind, required String? cli}) => kind != 'nup' && cli != null;

/// A NUP the engine has just read continues in build.all, DLSS included.
bool rustNupBuildApplies({required String? cli}) => cli != null;

/// A game folder keeps the exefs and romfs an update builds from again.
bool rustUpdateReady(String folder) =>
    Directory(p.join(folder, '.hookshot_exefs')).existsSync() && Directory(p.join(folder, 'romfs')).existsSync();

/// The language a built folder was made in (language.txt's code line).
String folderLanguage(String folder) {
  final file = File(p.join(folder, 'language.txt'));
  if (!file.existsSync()) return 'en-US';
  for (final line in file.readAsLinesSync()) {
    final code = line.trim();
    if (code.isNotEmpty && !code.startsWith('#')) return code;
  }
  return 'en-US';
}

/// The build.all request for an update: the folder's kept exefs/romfs, in place.
String rustUpdateRequest({
  required String folder,
  required String language,
  required bool prepare,
  bool dlss = false,
  String ngx = '',
  String vk = '',
}) =>
    rustBuildRequest(
      exefs: p.join(folder, '.hookshot_exefs'),
      romfs: p.join(folder, 'romfs'),
      out: folder,
      work: rustBuildWork(),
      language: language,
      prepare: prepare,
      dlss: dlss,
      ngx: ngx,
      vk: vk,
    );

/// The build.all request for what build.prepare left: work/exefs and out/romfs.
String rustNupBuildRequest({
  required String out,
  required String work,
  required String language,
  required bool prepare,
  bool dlss = false,
  String ngx = '',
  String assets = '',
  String vk = '',
}) =>
    rustBuildRequest(
      exefs: p.join(work, 'exefs'),
      romfs: p.join(out, 'romfs'),
      out: out,
      work: work,
      assets: assets,
      vk: vk,
      language: language,
      prepare: prepare,
      dlss: dlss,
      ngx: ngx,
    );

/// The request `hookshot_engine --stream` takes to read a .nup into work/exefs
/// and out/romfs; build.all then builds from those two folders.
String rustPrepareRequest({
  required String nup,
  required String out,
  required String work,
}) =>
    jsonEncode({
      'command': 'build.prepare',
      'nup': nup,
      'out': out,
      'work': work,
    });

/// The request `hookshot_engine --stream` takes on stdin. `dlss` builds the
/// Vulkan backend with NVIDIA's SDK (`ngx`, the folder `build.all` fetches
/// into when its library or runtime is missing).
String rustBuildRequest({
  required String exefs,
  required String romfs,
  required String out,
  required String work,
  required String language,
  bool prepare = false,
  bool dlss = false,
  String ngx = '',
  String assets = '',
  String vk = '',
}) =>
    jsonEncode({
      'command': 'build.all',
      'exefs': exefs,
      'romfs': romfs,
      'out': out,
      'work': work,
      'assets': assets,
      'vk': vk,
      'language': language,
      'prepare': prepare,
      'dlss': dlss,
      'ngx': dlss ? ngx : '',
    });
