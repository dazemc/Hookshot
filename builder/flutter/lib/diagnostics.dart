// Builder diagnostics for issue reports: the Rust engine's JSON report and
// the HOS_TRACE switch the game reads from hookshot_env.txt.
import 'dart:convert';
import 'dart:io';

import 'package:path/path.dart' as p;

import 'config.dart' as cfg;
import 'engine.dart';

Map<String, Object?>? collect(String folder, {String? builder, String? game}) {
  final engine = RustEngine.open();
  if (engine == null) return null;
  return engine.call({
    'command': 'diagnostics',
    'folder': folder,
    'tools': cfg.toolsRoot(),
    'builder': builder ?? cfg.builderVersion,
    'game': game ?? '',
  });
}

String pretty(Map<String, Object?> report) => const JsonEncoder.withIndent('  ').convert(report);

bool traceNextRun(String folder) {
  final file = File(p.join(folder, 'hookshot_env.txt'));
  if (!file.existsSync()) return false;
  return file.readAsLinesSync().any((line) => line.trim() == 'HOS_TRACE=1');
}

/// Set or remove HOS_TRACE=1, keeping every other line in hookshot_env.txt.
void setTraceNextRun(String folder, bool enabled) {
  final file = File(p.join(folder, 'hookshot_env.txt'));
  final lines = file.existsSync() ? file.readAsLinesSync() : <String>[];
  final kept = lines.where((line) => !line.trim().startsWith('HOS_TRACE=')).toList();
  if (enabled) kept.add('HOS_TRACE=1');
  file.writeAsStringSync(kept.isEmpty ? '' : '${kept.join('\n')}\n');
}
