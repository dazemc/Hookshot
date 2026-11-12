// The Rust engine's build.all path: when it replaces make_game.py, and the one
// JSON request its --stream mode reads from stdin.
import 'dart:convert';
import 'dart:io';

import 'package:path/path.dart' as p;

import '../config.dart' as cfg;

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
    p.join(tools, 'builder', 'rust', 'target', 'release', name),
    p.join(tools, 'builder', 'rust', 'target', 'debug', name),
  ]) {
    if (File(path).existsSync()) return path;
  }
  return null;
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

/// build.all needs an unpacked exefs/romfs pair, and no shader preparation or
/// NVIDIA SDK download: those two are still make_game.py's.
bool rustBuildApplies({
  required String kind,
  required bool prepare,
  required bool dlss,
  required String? cli,
}) =>
    kind != 'nup' && !prepare && !dlss && cli != null;

/// The request `hookshot_engine --stream` takes on stdin.
String rustBuildRequest({
  required String exefs,
  required String romfs,
  required String out,
  required String work,
  required String language,
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
    });
