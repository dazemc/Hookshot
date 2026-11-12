// Whether an update keeps a game folder's NVIDIA DLSS. Upstream NotRexed/Hookshot b56bf07c (#2528): the DLSS SDK's
// LICENSE.txt was the only record that NVIDIA's license was accepted, so deleting the SDK (or a new builder folder)
// made an update build without DLSS and remove its runtime. The acceptance is kept in the builder's settings instead,
// with the SDK's file still accepted for a license recorded before that.

/// The builder settings' record that NVIDIA's license was accepted. `dlss` is the saved tick, which older builders
/// wrote only after the license dialog was accepted, so it is the record for a config from before `dlss_license`.
bool dlssLicenseAccepted(Map<String, String> config) => config['dlss_license'] == '1' || config['dlss'] == '1';

/// The settings after the DLSS tick changed: unticking keeps the accepted-license record, so a later update still
/// keeps a folder built with DLSS.
Map<String, String> dlssAfterToggle(Map<String, String> config, bool ticked) {
  final next = Map<String, String>.from(config);
  next['dlss'] = ticked ? '1' : '0';
  if (ticked) next['dlss_license'] = '1';
  return next;
}

/// Whether an update keeps DLSS: the tick, or a folder with DLSS's runtime whose license was accepted (the settings'
/// record, or the SDK's LICENSE.txt for a license accepted before the record). A folder without the runtime loses it,
/// as does one whose license was never accepted.
bool keepDlssOnUpdate({
  required bool ticked,
  required bool hasRuntime,
  required bool licenseAccepted,
  required bool sdkLicense,
}) =>
    ticked || (hasRuntime && (licenseAccepted || sdkLicense));

/// Whether an update builds with DLSS. An explicit tick change wins, so unticking and updating removes DLSS as the
/// first page says; otherwise the tick or the folder's runtime with the accepted license keeps it.
bool updateDlss({
  required bool toggled,
  required bool ticked,
  required bool hasRuntime,
  required bool licenseAccepted,
  required bool sdkLicense,
}) =>
    toggled
        ? ticked
        : keepDlssOnUpdate(
            ticked: ticked,
            hasRuntime: hasRuntime,
            licenseAccepted: licenseAccepted,
            sdkLicense: sdkLicense,
          );
