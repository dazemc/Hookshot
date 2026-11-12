import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/backend/rust_build.dart';
import 'package:path/path.dart' as p;

void main() {
  group('rustEngineCli', () {
    test(
      'finds a CLI in the checkout, or the one named by the environment',
      () async {
        final tmp = await Directory.systemTemp.createTemp('hookshot-rust-cli-');
        addTearDown(() => tmp.delete(recursive: true));
        final name = Platform.isWindows ? 'hookshot_engine.exe' : 'hookshot_engine';
        final cli = File(
          p.join(tmp.path, 'builder', 'rust', 'target', 'release', name),
        );
        cli.createSync(recursive: true);

        expect(rustEngineCli(root: tmp.path, exeDir: tmp.path), cli.path);
        expect(
          rustEngineCli(override: cli.path, root: tmp.path, exeDir: tmp.path),
          cli.path,
        );
      },
    );

    test('a missing CLI keeps the Python build', () async {
      final tmp = await Directory.systemTemp.createTemp(
        'hookshot-no-rust-cli-',
      );
      addTearDown(() => tmp.delete(recursive: true));
      expect(
        rustEngineCli(override: '', root: tmp.path, exeDir: tmp.path),
        isNull,
      );
    });
  });

  group('rustBuildApplies', () {
    test('only an unpacked folder without shader preparation or DLSS', () {
      expect(
        rustBuildApplies(
          kind: 'folders',
          prepare: false,
          dlss: false,
          cli: '/engine',
        ),
        isTrue,
      );
      expect(
        rustBuildApplies(
          kind: 'nup',
          prepare: false,
          dlss: false,
          cli: '/engine',
        ),
        isFalse,
      );
      expect(
        rustBuildApplies(
          kind: 'folders',
          prepare: true,
          dlss: false,
          cli: '/engine',
        ),
        isFalse,
      );
      expect(
        rustBuildApplies(
          kind: 'folders',
          prepare: false,
          dlss: true,
          cli: '/engine',
        ),
        isFalse,
      );
      expect(
        rustBuildApplies(
          kind: 'folders',
          prepare: false,
          dlss: false,
          cli: null,
        ),
        isFalse,
      );
    });
  });

  test('the request names build.all and every folder', () {
    final request = jsonDecode(
      rustBuildRequest(
        exefs: '/games/exefs',
        romfs: '/games/romfs',
        out: '/games/Hookshot',
        work: '/cache/hookshot-builder',
        language: 'fr',
      ),
    );
    expect(request, {
      'command': 'build.all',
      'exefs': '/games/exefs',
      'romfs': '/games/romfs',
      'out': '/games/Hookshot',
      'work': '/cache/hookshot-builder',
      'assets': '',
      'vk': '',
      'language': 'fr',
    });
  });
}
