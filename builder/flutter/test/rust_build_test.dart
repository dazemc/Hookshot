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
    test('an unpacked folder with the engine, DLSS included', () {
      expect(
        rustBuildApplies(kind: 'folders', cli: '/engine'),
        isTrue,
      );
      expect(
        rustBuildApplies(kind: 'nup', cli: '/engine'),
        isFalse,
      );
      expect(
        rustBuildApplies(kind: 'folders', cli: null),
        isFalse,
      );
    });
  });

  group('DLSS', () {
    test('a Windows DLSS build is a typed error, not a Python fallback', () {
      expect(
        () => requireDlssSupported(dlss: true, windows: true),
        throwsA(isA<DlssBuildUnsupported>()),
      );
      expect(
        () => requireDlssSupported(dlss: true, windows: false),
        returnsNormally,
      );
      expect(
        () => requireDlssSupported(dlss: false, windows: true),
        returnsNormally,
      );
    });

    test('the SDK folder follows each platform', () {
      expect(
        dlssSdkDir(
          windows: false,
          home: '/home/z',
          xdgDataHome: '',
        ),
        p.join('/home/z', '.local', 'share', 'hookshot', 'builder', 'ngx-sdk'),
      );
      expect(
        dlssSdkDir(
          windows: false,
          home: '/home/z',
          xdgDataHome: '/data',
        ),
        p.join('/data', 'hookshot', 'builder', 'ngx-sdk'),
      );
      final windows = dlssSdkDir(
        windows: true,
        localAppData: r'C:\Users\z\AppData\Local',
        home: r'C:\Users\z',
      );
      expect(windows, contains('AppData'));
      expect(windows, endsWith(p.join('HookshotBuilder', 'ngx-sdk')));
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

    test('a read NUP continues in build.all, DLSS included', () {
      expect(rustNupBuildApplies(cli: '/engine'), isTrue);
      expect(rustNupBuildApplies(cli: null), isFalse);
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
        'dlss': false,
        'ngx': '',
      });
      final dlss = jsonDecode(
        rustNupBuildRequest(
          out: '/games/Hookshot',
          work: '/cache/hookshot-builder',
          language: 'fr',
          prepare: true,
          dlss: true,
          ngx: '/home/z/.local/share/hookshot/builder/ngx-sdk',
        ),
      );
      expect(dlss['dlss'], isTrue);
      expect(dlss['ngx'], '/home/z/.local/share/hookshot/builder/ngx-sdk');
      expect(dlss['prepare'], isTrue);
    });
  });

  group('updates', () {
    test('a kept game folder is updated by build.all in place', () {
      final request = jsonDecode(
        rustUpdateRequest(
          folder: '/games/Hookshot',
          language: 'de',
          prepare: true,
        ),
      );
      expect(request, {
        'command': 'build.all',
        'exefs': '/games/Hookshot/.hookshot_exefs',
        'romfs': '/games/Hookshot/romfs',
        'out': '/games/Hookshot',
        'work': request['work'],
        'assets': '',
        'vk': '',
        'language': 'de',
        'prepare': true,
        'dlss': false,
        'ngx': '',
      });
    });

    test('a kept folder is recognised and its language read', () {
      final tmp = Directory.systemTemp.createTempSync('hookshot-update-');
      addTearDown(() => tmp.deleteSync(recursive: true));
      Directory(p.join(tmp.path, '.hookshot_exefs')).createSync();
      Directory(p.join(tmp.path, 'romfs')).createSync();
      File(p.join(tmp.path, 'language.txt')).writeAsStringSync('# the language\n\nfr\n\n# de  German\n');
      expect(rustUpdateReady(tmp.path), isTrue);
      expect(folderLanguage(tmp.path), 'fr');
      Directory(p.join(tmp.path, '.hookshot_exefs')).deleteSync(recursive: true);
      expect(rustUpdateReady(tmp.path), isFalse);
      expect(folderLanguage(tmp.path), 'fr');
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
      'dlss': false,
      'ngx': '',
    });
  });

  test('a DLSS build names its SDK folder', () {
    final request = jsonDecode(
      rustBuildRequest(
        exefs: '/games/exefs',
        romfs: '/games/romfs',
        out: '/games/Hookshot',
        work: '/cache/hookshot-builder',
        language: 'fr',
        dlss: true,
        ngx: '/home/z/.local/share/hookshot/builder/ngx-sdk',
      ),
    );
    expect(request['dlss'], isTrue);
    expect(request['ngx'], '/home/z/.local/share/hookshot/builder/ngx-sdk');
  });
}
