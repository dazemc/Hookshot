import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/semantics.dart';
import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/a11y.dart';
import 'package:hookshot_builder/blocs/builder_bloc.dart';
import 'package:hookshot_builder/blocs/launcher_bloc.dart';
import 'package:hookshot_builder/pages/files_page.dart';
import 'package:hookshot_builder/pages/launcher_page.dart';
import 'package:hookshot_builder/pages/progress_page.dart';
import 'package:hookshot_builder/pages/ready_page.dart';
import 'package:hookshot_builder/pages/widgets.dart';
import 'package:hookshot_builder/theme.dart';

class _FixedBuilderBloc extends BuilderBloc {
  final BuilderState fixedState;
  _FixedBuilderBloc(this.fixedState) : super();

  @override
  BuilderState get state => fixedState;

  @override
  Stream<BuilderState> get stream => const Stream<BuilderState>.empty();

  @override
  void add(BuilderEvent event) {}
}

class _FixedLauncherBloc extends LauncherBloc {
  final LauncherState fixedState;
  _FixedLauncherBloc(this.fixedState) : super('/games/Hookshot');

  @override
  LauncherState get state => fixedState;

  @override
  Stream<LauncherState> get stream => const Stream<LauncherState>.empty();

  @override
  void add(LauncherEvent event) {}
}

BuilderForm _form() => const BuilderForm(
      kind: 'nup',
      nup: '/games/example.nup',
      romfs: '/games/romfs',
      exefs: '/games/exefs',
      out: '/games/Hookshot',
      language: 'English (US)',
      prepare: true,
      dlss: false,
      track: 'release',
      checkText: '',
      latest: null,
      spaceGb: 120,
      buttonLabel: 'Build the game',
      aboutLabel: 'About 45 min',
      gameReady: false,
      sourcePicked: false,
      dlssToggled: false,
      mode: 'build',
    );

BuildRunning _running() => BuildRunning(
      title: 'Building your game',
      stageText: 'Compiling 7 of 10 changed files',
      fraction: 0.62,
      eta: 120,
      log: const ['Build output line 0', 'Build output line 1'],
      changed: 10,
      compiledFiles: 7,
      totalFiles: 10,
      started: DateTime.now().subtract(const Duration(minutes: 2)),
      compileStarted: DateTime.now().subtract(const Duration(minutes: 1)),
      totalSecs: null,
      compiledSecs: null,
      previousSecs: null,
      shadersLater: false,
      damagedCount: 0,
      error: null,
      built: null,
    );

Widget _builderApp(BuilderState state) => FluentApp(
      theme: hsTheme(),
      home: BlocProvider<BuilderBloc>.value(
        value: _FixedBuilderBloc(state),
        child: switch (state) {
          BuildRunning() => const ProgressPage(),
          BuildDone() => const ReadyPage(),
          BuildFailed() => const ProblemPage(),
          _ => const FilesPage(),
        },
      ),
    );

Widget _launcherApp(LauncherState state) => FluentApp(
      theme: hsTheme(),
      home: BlocProvider<LauncherBloc>.value(
        value: _FixedLauncherBloc(state),
        child: const LauncherPage(toolsTag: 'v1.0.0'),
      ),
    );

void _useSize(WidgetTester tester, Size size) {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}

double _contrast(Color a, Color b) {
  final la = a.computeLuminance();
  final lb = b.computeLuminance();
  final lighter = la > lb ? la : lb;
  final darker = la > lb ? lb : la;
  return (lighter + 0.05) / (darker + 0.05);
}

void main() {
  group('semantics labels', () {
    testWidgets('source choices expose a button role and selected state', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_builderApp(_form()));
      await tester.pump();

      final node = tester.getSemantics(find.bySemanticsLabel('.nup package. A single game file'));
      final data = node.getSemanticsData();
      expect(data.hasFlag(SemanticsFlag.isButton), isTrue);
      expect(data.hasFlag(SemanticsFlag.isSelected), isTrue);
      expect(data.hasAction(SemanticsAction.tap), isTrue);
      semantics.dispose();
    });

    testWidgets('option rows merge the checkbox and its visible label', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_builderApp(_form()));
      await tester.pump();

      final node = tester.getSemantics(find.text('Prepare shaders before first launch'));
      final data = node.getSemanticsData();
      expect(data.label, contains('Prepare shaders before first launch'));
      expect(data.hasFlag(SemanticsFlag.hasCheckedState), isTrue);
      expect(data.hasFlag(SemanticsFlag.isChecked), isTrue);
      semantics.dispose();
    });

    testWidgets('path fields keep their visible label', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_builderApp(_form()));
      await tester.pump();

      final field = tester.getSemantics(find.byType(TextBox).first);
      expect(field.getSemanticsData().label, contains('Package'));
      semantics.dispose();
    });

    testWidgets('the step pill names the current step', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_builderApp(const BuildDone(
        folder: '/games/Hookshot',
        text: 'Ready in 00:40',
        compiledSecs: 10,
        previousSecs: null,
        shadersLater: false,
        damagedCount: 0,
      )));
      await tester.pump();

      expect(find.bySemanticsLabel('Step 3 of 3: Play'), findsOneWidget);
      semantics.dispose();
    });

    testWidgets('the launcher loading ring and status are labelled', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_launcherApp(const LauncherLoading('/games/Hookshot')));
      await tester.pump();

      expect(find.bySemanticsLabel('Checking your game'), findsOneWidget);
      final status = tester.getSemantics(find.bySemanticsLabel('Checking for updates…'));
      expect(status.getSemanticsData().hasFlag(SemanticsFlag.isLiveRegion), isTrue);
      semantics.dispose();
    });
  });

  group('progress announcements', () {
    testWidgets('the build bar exposes its value and the stage is a live region', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_builderApp(_running()));
      await tester.pump();

      final progress = tester.getSemantics(find.bySemanticsLabel('Build progress'));
      expect(progress.getSemanticsData().value, '62%');

      final stage = tester.getSemantics(find.bySemanticsLabel('Compiling 7 of 10 changed files'));
      expect(stage.getSemanticsData().hasFlag(SemanticsFlag.isLiveRegion), isTrue);
      semantics.dispose();
    });

    testWidgets('the stage track says which stages are done and current', (tester) async {
      final semantics = tester.ensureSemantics();
      _useSize(tester, const Size(1000, 720));
      await tester.pumpWidget(_builderApp(_running()));
      await tester.pump();

      expect(find.bySemanticsLabel('Read game files, done'), findsOneWidget);
      expect(find.bySemanticsLabel('Compile changed files, in progress'), findsOneWidget);
      expect(find.bySemanticsLabel('Assemble game folder, not started'), findsOneWidget);
      semantics.dispose();
    });

    testWidgets('a live status line announces changes without duplicating text', (tester) async {
      final semantics = tester.ensureSemantics();
      await tester.pumpWidget(FluentApp(
        theme: hsTheme(),
        home: const ScaffoldPage(
          content: HsStatusLine('Checking for updates…', live: true),
        ),
      ));

      final node = tester.getSemantics(find.bySemanticsLabel('Checking for updates…'));
      final data = node.getSemanticsData();
      expect(data.hasFlag(SemanticsFlag.isLiveRegion), isTrue);
      expect(data.label, 'Checking for updates…');
      semantics.dispose();
    });

    testWidgets('a live error banner announces its title and details', (tester) async {
      final semantics = tester.ensureSemantics();
      await tester.pumpWidget(FluentApp(
        theme: hsTheme(),
        home: const ScaffoldPage(
          content: HsBanner(
            kind: HsBannerKind.warning,
            title: 'Error details',
            message: 'The download stopped.',
            live: true,
          ),
        ),
      ));

      final node = tester.getSemantics(find.bySemanticsLabel('Error details. The download stopped.'));
      expect(node.getSemanticsData().hasFlag(SemanticsFlag.isLiveRegion), isTrue);
      semantics.dispose();
    });
  });

  group('contrast tokens', () {
    test('faint and soft text meet WCAG AA on the page and cards', () {
      expect(_contrast(HsColors.inkFaint, HsColors.page), greaterThanOrEqualTo(4.5));
      expect(_contrast(HsColors.inkFaint, HsColors.raised), greaterThanOrEqualTo(4.5));
      expect(_contrast(HsColors.inkSoft, HsColors.page), greaterThanOrEqualTo(4.5));
      expect(_contrast(HsColors.inkSoft, HsColors.raised), greaterThanOrEqualTo(4.5));
      expect(_contrast(HsColors.ink, HsColors.page), greaterThanOrEqualTo(4.5));
    });

    test('the focus ring stands out on the page, cards and primary buttons', () {
      expect(_contrast(HsColors.focusOuter, HsColors.page), greaterThanOrEqualTo(3));
      expect(_contrast(HsColors.focusOuter, HsColors.raised), greaterThanOrEqualTo(3));
      expect(_contrast(HsColors.focusOuter, HsColors.brand), greaterThanOrEqualTo(3));
    });

    test('the theme carries the focus ring', () {
      final focus = hsTheme().focusTheme;
      expect(focus.primaryBorder?.color, HsColors.focusOuter);
      expect(focus.primaryBorder?.width, 2);
      expect(focus.renderOutside, isTrue);
    });

    testWidgets('a focused button paints the ring', (tester) async {
      final previousStrategy = FocusManager.instance.highlightStrategy;
      FocusManager.instance.highlightStrategy = FocusHighlightStrategy.alwaysTraditional;
      addTearDown(() => FocusManager.instance.highlightStrategy = previousStrategy);
      final focusNode = FocusNode();
      addTearDown(focusNode.dispose);
      await tester.pumpWidget(FluentApp(
        theme: hsTheme(),
        home: ScaffoldPage(
          content: FilledButton(
            focusNode: focusNode,
            onPressed: () {},
            child: const Text('Play'),
          ),
        ),
      ));
      focusNode.requestFocus();
      await tester.pumpAndSettle();

      final borders = tester.widgetList<FocusBorder>(find.byType(FocusBorder));
      expect(borders.any((border) => border.focused), isTrue);
    });
  });

  group('reduced motion', () {
    testWidgets('transitions collapse to zero duration', (tester) async {
      late Duration normal;
      late Duration reduced;
      await tester.pumpWidget(MediaQuery(
        data: const MediaQueryData(),
        child: Builder(builder: (context) {
          normal = HsMotion.duration(context, 240);
          return const SizedBox.shrink();
        }),
      ));
      await tester.pumpWidget(MediaQuery(
        data: const MediaQueryData(disableAnimations: true),
        child: Builder(builder: (context) {
          reduced = HsMotion.duration(context, 240);
          return const SizedBox.shrink();
        }),
      ));
      expect(normal, const Duration(milliseconds: 240));
      expect(reduced, Duration.zero);
    });

    testWidgets('the status pop does not animate', (tester) async {
      await tester.pumpWidget(const MediaQuery(
        data: MediaQueryData(disableAnimations: true),
        child: Directionality(
          textDirection: TextDirection.ltr,
          child: HsPopIn(child: SizedBox(width: 10, height: 10)),
        ),
      ));

      final animation = tester.widget<TweenAnimationBuilder<double>>(find.byType(TweenAnimationBuilder<double>));
      expect(animation.duration, Duration.zero);
    });

    testWidgets('the step pill changes without animating', (tester) async {
      await tester.pumpWidget(FluentApp(
        theme: hsTheme(),
        home: const MediaQuery(
          data: MediaQueryData(disableAnimations: true),
          child: ScaffoldPage(content: StepsPill(on: 1)),
        ),
      ));

      final containers = tester.widgetList<AnimatedContainer>(find.byType(AnimatedContainer));
      expect(containers, isNotEmpty);
      expect(containers.every((container) => container.duration == Duration.zero), isTrue);
    });
  });
}
