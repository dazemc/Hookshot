import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/backend/rust_build.dart';
import 'package:hookshot_builder/engine.dart';
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

    test('finds the CLI bundled beside the app libraries', () async {
      final tmp = await Directory.systemTemp.createTemp('hookshot-rust-lib-');
      addTearDown(() => tmp.delete(recursive: true));
      final name = Platform.isWindows ? 'hookshot_engine.exe' : 'hookshot_engine';
      final cli = File(p.join(tmp.path, 'lib', name));
      cli.createSync(recursive: true);

      expect(rustEngineCli(override: '', root: tmp.path, exeDir: tmp.path), cli.path);
    });

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

    test('a missing CLI is a typed error for a NUP build', () async {
      final tmp = await Directory.systemTemp.createTemp(
        'hookshot-no-rust-nup-',
      );
      addTearDown(() => tmp.delete(recursive: true));
      expect(
        () => requireRustEngineCli(
          override: '',
          root: tmp.path,
          exeDir: tmp.path,
        ),
        throwsA(isA<RustEngineMissing>()),
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
        isTrue,
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

  group('NUP routing', () {
    test('only a NUP input is read by the engine first', () {
      expect(rustNupApplies(kind: 'nup'), isTrue);
      expect(rustNupApplies(kind: 'folders'), isFalse);
    });

    test('the prepare request names build.prepare and its folders', () {
      final request = jsonDecode(
        rustPrepareRequest(
          nup: '/games/My Game.nup',
          out: '/games/Hookshot',
          work: '/cache/hookshot-builder',
        ),
      );
      expect(request, {
        'command': 'build.prepare',
        'nup': '/games/My Game.nup',
        'out': '/games/Hookshot',
        'work': '/cache/hookshot-builder',
      });
    });

    test('a read NUP continues in build.all when no Python stage is needed', () {
      expect(
        rustNupBuildApplies(prepare: false, dlss: false, cli: '/engine'),
        isTrue,
      );
      expect(
        rustNupBuildApplies(prepare: true, dlss: false, cli: '/engine'),
        isTrue,
      );
      expect(
        rustNupBuildApplies(prepare: false, dlss: true, cli: '/engine'),
        isFalse,
      );
      expect(
        rustNupBuildApplies(prepare: false, dlss: false, cli: null),
        isFalse,
      );
      final request = jsonDecode(
        rustNupBuildRequest(
          out: '/games/Hookshot',
          work: '/cache/hookshot-builder',
          language: 'fr',
          prepare: false,
        ),
      );
      expect(request, {
        'command': 'build.all',
        'exefs': '/cache/hookshot-builder/exefs',
        'romfs': '/games/Hookshot/romfs',
        'out': '/games/Hookshot',
        'work': '/cache/hookshot-builder',
        'assets': '',
        'vk': '',
        'language': 'fr',
        'prepare': false,
      });
    });

    test('the Python stage after the read skips reading the .nup again', () {
      expect(
        rustNupPythonArgs(
          out: '/games/Hookshot',
          work: '/cache/hookshot-builder',
          language: 'fr',
        ),
        [
          '--already-read',
          '--out',
          '/games/Hookshot',
          '--work',
          '/cache/hookshot-builder',
          '--language',
          'fr',
        ],
      );
      expect(
        rustNupPythonArgs(
          out: '/games/Hookshot',
          work: '/cache/hookshot-builder',
          language: 'fr',
          jobs: '4',
          prepare: false,
          dlss: true,
        ),
        [
          '--already-read',
          '--out',
          '/games/Hookshot',
          '--work',
          '/cache/hookshot-builder',
          '--language',
          'fr',
          '--jobs',
          '4',
          '--no-prepare-shaders',
          '--fetch-ngx-sdk',
          '--accept-nvidia-license',
        ],
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
      'prepare': false,
    });
  });
}
