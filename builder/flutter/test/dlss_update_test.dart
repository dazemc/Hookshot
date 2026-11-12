// The Windows builder's DLSS record, ported from upstream NotRexed/Hookshot b56bf07c (#2528): an update keeps a
// folder's DLSS when its runtime is there and NVIDIA's license was accepted, even after the SDK's files are gone.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/updater/dlss.dart';

void main() {
  group('the accepted-license record', () {
    test('is the saved tick for a config from before dlss_license', () {
      expect(dlssLicenseAccepted({'dlss': '1'}), isTrue);
      expect(dlssLicenseAccepted({'dlss': '0'}), isFalse);
      expect(dlssLicenseAccepted({}), isFalse);
    });

    test('survives unticking DLSS', () {
      final accepted = dlssAfterToggle({'dlss': '0'}, true);
      expect(accepted['dlss'], '1');
      expect(accepted['dlss_license'], '1');
      final unticked = dlssAfterToggle(accepted, false);
      expect(unticked['dlss'], '0');
      expect(unticked['dlss_license'], '1');
      expect(dlssLicenseAccepted(unticked), isTrue);
    });

    test('is not written when the license is declined', () {
      final declined = dlssAfterToggle({}, false);
      expect(declined['dlss'], '0');
      expect(declined.containsKey('dlss_license'), isFalse);
      expect(dlssLicenseAccepted(declined), isFalse);
    });
  });

  group('an update keeps DLSS', () {
    test('when it is ticked, even for a folder not built with it yet', () {
      expect(
        keepDlssOnUpdate(ticked: true, hasRuntime: false, licenseAccepted: false, sdkLicense: false),
        isTrue,
      );
    });

    test('from the accepted-license record when the SDK is gone', () {
      expect(
        keepDlssOnUpdate(ticked: false, hasRuntime: true, licenseAccepted: true, sdkLicense: false),
        isTrue,
      );
    });

    test('from the SDK LICENSE.txt for a license accepted before the record', () {
      expect(
        keepDlssOnUpdate(ticked: false, hasRuntime: true, licenseAccepted: false, sdkLicense: true),
        isTrue,
      );
    });

    test('only when the folder still has DLSS runtime', () {
      expect(
        keepDlssOnUpdate(ticked: false, hasRuntime: false, licenseAccepted: true, sdkLicense: true),
        isFalse,
      );
      expect(
        keepDlssOnUpdate(ticked: false, hasRuntime: true, licenseAccepted: false, sdkLicense: false),
        isFalse,
      );
    });

    test('unless the player explicitly changed the tick', () {
      expect(
        updateDlss(toggled: true, ticked: false, hasRuntime: true, licenseAccepted: true, sdkLicense: true),
        isFalse,
      );
      expect(
        updateDlss(toggled: true, ticked: true, hasRuntime: false, licenseAccepted: false, sdkLicense: false),
        isTrue,
      );
      expect(
        updateDlss(toggled: false, ticked: false, hasRuntime: true, licenseAccepted: true, sdkLicense: false),
        isTrue,
      );
    });
  });

  test('the builder writes the record and uses it for an update', () {
    final source = File('lib/blocs/builder_bloc.dart').readAsStringSync();
    expect(source.contains('dlss.dlssAfterToggle(_cfg, e.value)'), isTrue,
        reason: 'the tick must keep the accepted-license record');
    expect(source.contains('dlss.dlssLicenseAccepted(_cfg)'), isTrue,
        reason: 'a folder with DLSS runtime must be read through the record');
    expect(source.contains('updateDlss('), isTrue);
    expect(
      source.contains('ticked: s is BuilderForm && s.dlss'),
      isTrue,
      reason: 'an update must keep a folder built with DLSS even when the tick is off, unless the player changed it',
    );
    expect(source.contains('toggled: s is BuilderForm && s.dlssToggled'), isTrue);
  });
}
