// The NUP route's process handoff: the request Flutter writes is the one the
// engine's --stream mode reads, and its first stage is the read stage.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/backend/process.dart';
import 'package:hookshot_builder/backend/rust_build.dart';
import 'package:path/path.dart' as p;

void main() {
  test('a missing .nup stops in the engine read stage, before Python', () async {
    final cli = rustEngineCli();
    if (cli == null) {
      markTestSkipped('the Rust engine CLI is not built');
      return;
    }
    final tmp = await Directory.systemTemp.createTemp('hookshot-rust-nup-route-');
    addTearDown(() => tmp.delete(recursive: true));
    final process = await BuildProcess.start(
      [cli, '--stream'],
      cwd: tmp.path,
      stdinText: rustPrepareRequest(
        nup: p.join(tmp.path, 'not-the-game.nup'),
        out: p.join(tmp.path, 'game'),
        work: p.join(tmp.path, 'work'),
      ),
    );
    final lines = await process.lines.toList();
    expect(await process.exitCode, 1);
    expect(lines.first, '@@stage read Reading your game files');
    expect(lines[1], '@@progress 0.000');
    expect(
      lines.any(
          (line) => line.startsWith('@@error Could not access your .nup file ') && line.contains('not-the-game.nup')),
      isTrue,
      reason: lines.join('\n'),
    );
    expect(lines.any((line) => line.startsWith('@@done ')), isFalse);
  });
}
