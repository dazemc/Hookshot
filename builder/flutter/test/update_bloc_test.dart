import 'dart:convert';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/blocs/update_bloc.dart';
import 'package:hookshot_builder/updater/installer.dart';

List<int> _zip(Map<String, String> entries) {
  final archive = Archive();
  entries.forEach((name, content) {
    archive.addFile(ArchiveFile(name, content.length, utf8.encode(content))..mode = 33188);
  });
  return ZipEncoder().encode(archive);
}

void main() {
  group('UpdateBloc version admission', () {
    const sha = '0000000000000000000000000000000000000000000000000000000000000000';
    const list = 'tag v9.9.8\npart $sha 1 . https://example.com/a\ntree\n';

    test('a release list that lags the requested target is refused', () async {
      final tmp = await Directory.systemTemp.createTemp('hookshot-bloc-lag-');
      try {
        final bloc = UpdateBloc(
          fetcher: (url, {stop}) async => utf8.encode(list),
          runningRoot: () => tmp.path,
          installRoot: '${tmp.path}/builder',
        );
        addTearDown(bloc.close);
        final failed = expectLater(
            bloc.stream,
            emitsThrough(
                isA<UpdateFailed>().having((s) => s.error, 'error', contains('GitHub offered v9.9.8, not v9.9.9'))));
        bloc.add(const BeginUpdate(folder: '', toolsTag: 'v9.9.8', latest: 'v9.9.9'));
        await failed;
        expect(Directory('${tmp.path}/builder/versions/v9.9.8').existsSync(), isFalse);
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('a fetched builder that is not the requested version is refused', () async {
      final bloc =
          UpdateBloc(fetcher: (url, {stop}) async => throw const SocketException('offline'), runningRoot: () => null);
      addTearDown(bloc.close);
      final failed = expectLater(
          bloc.stream,
          emitsThrough(isA<UpdateFailed>()
              .having((s) => s.error, 'error', contains('This Builder is v9.9.8, not the requested v9.9.9'))));
      bloc.add(const BeginUpdate(folder: '', toolsTag: 'v9.9.8', latest: 'v9.9.9', tag: 'v9.9.9', fetched: true));
      await failed;
    });
  });

  group('UpdateBloc cancellation', () {
    test('a stopped update returns to an idle form', () async {
      final bloc =
          UpdateBloc(fetcher: (url, {stop}) async => throw const SocketException('offline'), runningRoot: () => null);
      addTearDown(bloc.close);
      final failed = expectLater(bloc.stream, emitsThrough(isA<UpdateFailed>()));
      bloc.add(const BeginUpdate(folder: '', toolsTag: 'v9.9.0', latest: 'v9.9.9'));
      await failed;
      final idle = expectLater(bloc.stream, emitsThrough(isA<UpdateIdle>()));
      bloc.add(const UpdateReset());
      await idle;
    });

    test('cancelling a download stops the update and stages nothing', () async {
      final tmp = await Directory.systemTemp.createTemp('hookshot-bloc-cancel-');
      try {
        final treeZip = _zip({'usr/share/hookshot/builder/linux/AppRun': '#!/bin/sh\n'});
        final treeSha = sha256.convert(treeZip).toString();
        final list = 'tag v9.9.9\npart $treeSha ${treeZip.length} . https://example.com/tree.zip\ntree\n';
        Future<List<int>> get(String url, {StopFlag? stop}) async {
          if (url.endsWith('hookshot-linux.txt')) return utf8.encode(list);
          for (var i = 0; i < 200; i++) {
            await Future<void>.delayed(const Duration(milliseconds: 20));
            if (stop != null && stop.isSet) throw const Cancelled();
          }
          return treeZip;
        }

        final bloc = UpdateBloc(
          fetcher: get,
          runningRoot: () => tmp.path,
          installRoot: '${tmp.path}/builder',
        );
        addTearDown(bloc.close);
        final done = expectLater(
            bloc.stream, emitsThrough(isA<UpdateFailed>().having((s) => s.error, 'error', contains('cancelled'))));
        bloc.add(const BeginUpdate(folder: '', toolsTag: 'v9.9.0', latest: 'v9.9.9'));
        await Future<void>.delayed(const Duration(milliseconds: 400));
        bloc.add(const CancelUpdate());
        await done;
        expect(Directory('${tmp.path}/builder/versions/v9.9.9').existsSync(), isFalse);
        expect(Directory('${tmp.path}/builder/versions/v9.9.9.part').existsSync(), isFalse);
      } finally {
        await tmp.delete(recursive: true);
      }
    });
  });
}
