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

/// build.all needs an unpacked exefs/romfs pair. Shader preparation is Rust's
/// now; NVIDIA's DLSS SDK download is still make_game.py's.
bool rustBuildApplies({
  required String kind,
  required bool prepare,
  required bool dlss,
  required String? cli,
}) =>
    kind != 'nup' && !dlss && cli != null;

/// A NUP the engine has just read can continue in build.all; only the DLSS
/// SDK download still needs make_game.py.
bool rustNupBuildApplies({
  required bool prepare,
  required bool dlss,
  required String? cli,
}) =>
    !dlss && cli != null;

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
  String vk = '',
}) =>
    rustBuildRequest(
      exefs: p.join(folder, '.hookshot_exefs'),
      romfs: p.join(folder, 'romfs'),
      out: folder,
      work: rustBuildWork(),
      language: language,
      prepare: prepare,
      vk: vk,
    );

/// The build.all request for what build.prepare left: work/exefs and out/romfs.
String rustNupBuildRequest({
  required String out,
  required String work,
  required String language,
  required bool prepare,
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
    );

/// The request `hookshot_engine --stream` takes to read a .nup into work/exefs
/// and out/romfs for make_game.py's remaining stages.
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

/// make_game.py's arguments once the engine read the .nup: its read stage is
/// skipped and the already extracted files are used.
List<String> rustNupPythonArgs({
  required String out,
  required String work,
  required String language,
  String? jobs,
  bool prepare = true,
  bool dlss = false,
}) =>
    [
      '--already-read',
      '--out',
      out,
      '--work',
      work,
      '--language',
      language,
      if (jobs != null) ...['--jobs', jobs],
      if (!prepare) '--no-prepare-shaders',
      if (dlss) ...['--fetch-ngx-sdk', '--accept-nvidia-license'],
    ];

/// The request `hookshot_engine --stream` takes on stdin.
String rustBuildRequest({
  required String exefs,
  required String romfs,
  required String out,
  required String work,
  required String language,
  bool prepare = false,
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
    });
