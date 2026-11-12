import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/profiles.dart';

ProcessResult _ok(Object? json) => ProcessResult(0, 0, jsonEncode(json), '');

const _list = {
  'user': '/home/player/.local/share/hookshot',
  'supported': true,
  'profiles': [
    {'id': '5a3c9e1f0b7d24680000000000000001', 'name': 'Player', 'default': true},
    {'id': '5a3c9e1f0b7d24680000000000000002', 'name': 'Link', 'default': false},
  ],
  'selected': '5a3c9e1f0b7d24680000000000000002',
};

void main() {
  group('profiles', () {
    test('reads the list and keeps the chosen profile', () {
      final profiles = Profiles.fromJson(_list);
      expect(profiles.supported, isTrue);
      expect(profiles.list.map((p) => p.name), ['Player', 'Link']);
      expect(profiles.byId('5a3c9e1f0b7d24680000000000000002')?.name, 'Link');
      expect(profiles.byId('missing'), isNull);
    });

    test('open save folder never makes an empty profile folder', () async {
      final root = await Directory.systemTemp.createTemp('hookshot-profile-dir-');
      addTearDown(() => root.delete(recursive: true));
      final profiles = Profiles.fromJson({
        ..._list,
        'user': '${root.path}/user',
      });
      expect(profiles.folderToShow('5a3c9e1f0b7d24680000000000000002'), isNull);
      Directory('${root.path}/user/save').createSync(recursive: true);
      expect(profiles.folderToShow('5a3c9e1f0b7d24680000000000000002'), '${root.path}/user/save');
    });

    test('service passes the commands and surfaces an engine error', () async {
      final calls = <List<String>>[];
      final service = ProfilesService(run: (args) async {
        calls.add(args);
        if (args.contains('remove')) {
          return ProcessResult(1, 1, jsonEncode({'error': 'The default profile cannot be removed.'}), '');
        }
        return _ok(_list);
      });
      final profiles = await service.load('/game');
      expect(calls.single, containsAll(['list', '/game']));
      expect(calls.single.any((arg) => arg.endsWith('profiles.py')), isTrue);
      expect(profiles.byId(profiles.selected)?.name, 'Link');
      await expectLater(service.remove('/game', profiles.selected), throwsA(isA<ProfileException>()));
    });

    test('an import names the chosen emulator user and never forces past a running game', () async {
      final calls = <Map<String, Object?>>[];
      final service = ProfilesService(call: (request) {
        calls.add(request);
        return {'text': 'Copied 2 slots'};
      });
      final profiles = Profiles.fromJson(_list);
      final out = await service.importSave('/game', profiles, profiles.selected, '/emu', sourceProfile: 'ABC');
      expect(out, 'Copied 2 slots');
      expect(calls.single['command'], 'saves.import');
      expect(calls.single['from'], '/emu');
      expect(calls.single['hookshot'], '/home/player/.local/share/hookshot/save/${profiles.selected}');
      expect(calls.single['profile'], 'ABC');
      expect(calls.single.containsKey('force'), isFalse);
    });

    test('lists the saves the engine found for the chosen folder', () async {
      final calls = <Map<String, Object?>>[];
      final service = ProfilesService(call: (request) {
        calls.add(request);
        return {
          'saves': [
            {
              'id': 'ABC',
              'path': '/emu/save/ABC',
              'user': 'Link',
              'label': 'Link (ABC…): 2 slots',
            }
          ]
        };
      });
      final saves = await service.saves('/emu');
      expect(calls.single, {'command': 'saves.list', 'folder': '/emu'});
      expect(saves.single.id, 'ABC');
      expect(saves.single.path, '/emu/save/ABC');
      expect(saves.single.user, 'Link');
      expect(saves.single.label, 'Link (ABC…): 2 slots');
    });
  });
}
