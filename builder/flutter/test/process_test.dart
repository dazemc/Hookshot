import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/backend/process.dart';

void main() {
  test('optional stdin reaches the process', () async {
    if (!Platform.isLinux) {
      markTestSkipped('needs cat and setsid');
      return;
    }
    final process = await BuildProcess.start(
      ['cat'],
      cwd: Directory.current.path,
      stdinText: 'hello\n',
    );
    expect(await process.lines.toList(), ['hello']);
    expect(await process.exitCode, 0);
  });
}
