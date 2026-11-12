import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/config.dart' as cfg;
import 'package:path/path.dart' as p;

Directory _temp(String name) {
  final folder = Directory.systemTemp.createTempSync('hookshot-$name-');
  addTearDown(() {
    try {
      folder.deleteSync(recursive: true);
    } catch (_) {}
  });
  return folder;
}

void _mark(String home) {
  File(p.join(home, cfg.portableMarker)).writeAsStringSync('${cfg.portableMarkerText}\n');
}

void main() {
  group('portable home', () {
    test('the standard per-user home is unchanged', () {
      expect(
        cfg.builderHome(
          environment: const {
            'HOME': '/home/z',
            'XDG_DATA_HOME': '',
            'LOCALAPPDATA': r'C:\Users\z\AppData\Local',
          },
          executable: '/opt/Hookshot/HookshotBuilder',
          windows: false,
        ),
        p.join('/home/z', '.local', 'share', 'hookshot', 'builder'),
      );
      expect(
        cfg.builderHome(
          environment: const {
            'HOME': r'C:\Users\z',
            'LOCALAPPDATA': r'C:\Users\z\AppData\Local',
          },
          executable: r'C:\Users\z\AppData\Local\HookshotBuilder\versions\v1\HookshotBuilder\HookshotBuilder.exe',
          windows: true,
        ),
        r'C:\Users\z\AppData\Local\HookshotBuilder',
      );
    });

    test('a marked home is found from the installed app and follows a move', () {
      final root = _temp('portable-move');
      final tree = Directory(p.join(root.path, 'Portable ü, tree'))..createSync();
      final home = Directory(p.join(tree.path, 'HookshotBuilder'))..createSync();
      _mark(home.path);
      final exe = p.join(home.path, 'versions', 'v1', 'HookshotBuilder', 'HookshotBuilder.exe');
      Directory(p.dirname(exe)).createSync(recursive: true);
      File(exe).writeAsStringSync('authored; never executed');

      final resolved = cfg.resolveBuilderHome(environment: const {}, executable: exe, windows: false);
      expect(resolved.root, home.path);
      expect(resolved.portable, isTrue);

      final moved = Directory(p.join(root.path, 'Relocated ü, tree'));
      tree.renameSync(moved.path);
      final movedHome = p.join(moved.path, 'HookshotBuilder');
      final movedExe = p.join(movedHome, 'versions', 'v1', 'HookshotBuilder', 'HookshotBuilder.exe');
      expect(
        cfg.builderHome(environment: const {}, executable: movedExe, windows: false),
        movedHome,
      );
    });

    test('the setup base is the home and a broken marker never selects AppData', () {
      final root = _temp('portable-base');
      final home = Directory(p.join(root.path, 'HookshotBuilder'))..createSync();
      _mark(home.path);
      expect(
        cfg.resolveBuilderHome(
          environment: {'HOOKSHOT_SETUP_BASE': home.path},
          executable: p.join(root.path, 'unrelated', 'app.exe'),
          windows: false,
        ).root,
        home.path,
      );
      expect(
        cfg.isPortable(
          environment: {'HOOKSHOT_SETUP_BASE': home.path},
          executable: p.join(root.path, 'unrelated', 'app.exe'),
          windows: false,
        ),
        isTrue,
      );

      File(p.join(home.path, cfg.portableMarker)).writeAsStringSync('broken');
      final broken = cfg.resolveBuilderHome(
        environment: {'HOOKSHOT_SETUP_BASE': home.path},
        executable: p.join(root.path, 'unrelated', 'app.exe'),
        windows: false,
      );
      expect(broken.root, home.path);
      expect(broken.portable, isFalse);
    });

    test('an explicitly portable process without its home stops', () {
      final root = _temp('portable-missing');
      expect(
        () => cfg.resolveBuilderHome(
          environment: const {'HOOKSHOT_BUILDER_PORTABLE': '1'},
          executable: p.join(root.path, 'app', 'HookshotBuilder.exe'),
          windows: false,
        ),
        throwsStateError,
      );
    });

    test('the earlier portable.txt layout still works', () {
      final root = _temp('portable-txt');
      final exe = p.join(root.path, 'HookshotBuilder.exe');
      File(exe).writeAsStringSync('authored');
      File(p.join(root.path, 'portable.txt')).writeAsStringSync('');
      expect(
        cfg.builderHome(environment: const {}, executable: exe, windows: false),
        p.join(root.path, 'data', 'hookshot', 'builder'),
      );
    });

    test('the default game folder sits beside a portable home', () {
      final root = _temp('portable-game');
      final home = Directory(p.join(root.path, 'HookshotBuilder'))..createSync();
      _mark(home.path);
      expect(
        cfg.defaultGameFolder(
          environment: {'HOOKSHOT_SETUP_BASE': home.path},
          executable: p.join(root.path, 'app.exe'),
          windows: false,
        ),
        p.join(root.path, 'Hookshot'),
      );
      final elsewhere = _temp('portable-game-standard');
      expect(
        cfg.defaultGameFolder(
          environment: const {'HOME': '/home/z'},
          executable: p.join(elsewhere.path, 'app.exe'),
          windows: false,
        ),
        p.join('/home/z', 'Games', 'Hookshot'),
      );
    });
  });

  group('portable paths', () {
    test('same-volume paths are relative; another volume stays absolute', () {
      expect(
        cfg.portableSavePath(r'C:\SSD\HookshotBuilder\versions\v1', r'C:\SSD\HookshotBuilder', windows: true),
        r'versions\v1',
      );
      expect(
        cfg.portableSavePath(r'C:\SSD\Hookshot', r'C:\SSD\HookshotBuilder', windows: true),
        r'..\Hookshot',
      );
      expect(
        cfg.portableSavePath(r'D:\Games\Hookshot', r'C:\SSD\HookshotBuilder', windows: true),
        r'D:\Games\Hookshot',
      );
      expect(
        cfg.portableLoadPath(r'versions\v1', r'C:\SSD\HookshotBuilder', windows: true),
        r'C:\SSD\HookshotBuilder\versions\v1',
      );
      expect(
        cfg.portableLoadPath(r'..\Hookshot', r'C:\SSD\HookshotBuilder', windows: true),
        r'C:\SSD\Hookshot',
      );
      expect(
        cfg.portableSavePath('/ssd/Builder/versions/v1', '/ssd/Builder'),
        'versions/v1',
      );
      expect(
        cfg.portableLoadPath('versions/v1', '/ssd/Builder'),
        '/ssd/Builder/versions/v1',
      );
    });

    test('a stored relative path follows the moved home', () {
      final root = _temp('portable-stored');
      final home = Directory(p.join(root.path, 'HookshotBuilder'))..createSync();
      _mark(home.path);
      final stored = cfg.portableSavePath(p.join(home.path, 'game', 'My Game.nup'), home.path);
      expect(stored, p.join('game', 'My Game.nup'));
      final moved = Directory(p.join(root.path, 'Moved ü'))..createSync();
      final movedHome = p.join(moved.path, 'HookshotBuilder');
      expect(
        cfg.portableLoadPath(stored, movedHome),
        p.join(movedHome, 'game', 'My Game.nup'),
      );
    });

    test('builder.ini keeps relative paths and gives absolute ones back', () {
      final root = _temp('portable-config');
      final home = Directory(p.join(root.path, 'HookshotBuilder'))..createSync();
      _mark(home.path);
      final game = Directory(p.join(root.path, 'Hookshot'))..createSync();
      final nup = p.join(home.path, 'game.nup');
      final file = p.join(root.path, 'builder.ini');
      cfg.saveConfig(
        {'out': game.path, 'nup': nup, 'language': 'en-US', 'dlss': '0'},
        path: file,
        home: home.path,
        windows: false,
      );
      final text = File(file).readAsStringSync();
      expect(text, contains('out=../Hookshot\n'));
      expect(text, contains('nup=game.nup\n'));
      expect(text, isNot(contains(root.path)));

      final loaded = cfg.loadConfig(path: file, home: home.path, windows: false);
      expect(loaded['out'], game.path);
      expect(loaded['nup'], nup);
      expect(loaded['language'], 'en-US');

      final moved = Directory(p.join(root.path, 'Moved ü'))..createSync();
      Directory(p.join(root.path, 'Hookshot')).renameSync(p.join(moved.path, 'Hookshot'));
      Directory(home.path).renameSync(p.join(moved.path, 'HookshotBuilder'));
      final movedHome = p.join(moved.path, 'HookshotBuilder');
      final after = cfg.loadConfig(path: file, home: movedHome, windows: false);
      expect(after['out'], p.join(moved.path, 'Hookshot'));
      expect(after['nup'], p.join(movedHome, 'game.nup'));
    });

    test('a standard install keeps its absolute paths', () {
      final root = _temp('standard-config');
      final file = p.join(root.path, 'builder.ini');
      final game = p.join(root.path, 'Hookshot');
      cfg.saveConfig({'out': game}, path: file, portable: false, windows: false);
      expect(File(file).readAsStringSync(), contains('out=$game\n'));
      expect(
        cfg.loadConfig(path: file, portable: false, windows: false)['out'],
        game,
      );
    });
  });
}
