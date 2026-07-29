$ErrorActionPreference = 'Stop'

function Require-Changed([string]$before, [string]$after, [string]$label) {
    if ($before -ceq $after) {
        throw "Required transformation did not match: $label"
    }
    return $after
}

$winhttpPath = 'src/winhttp.rs'
$winhttp = Get-Content $winhttpPath -Raw
$updated = $winhttp.Replace('Foundation::GetLastError', 'errhandlingapi::GetLastError')
$winhttp = Require-Changed $winhttp $updated 'GetLastError module'
$updated = $winhttp.Replace('Networking::WinHttp::{', 'winhttp::{')
$winhttp = Require-Changed $winhttp $updated 'WinHTTP module'
$updated = $winhttp.Replace('let mut handle_message = |message| {', 'let mut handle_message = |message: Message| {')
$winhttp = Require-Changed $winhttp $updated 'message closure type'

$publishMarker = $winhttp.IndexOf('fn publish_blocking')
if ($publishMarker -lt 0) {
    throw 'Could not find publish_blocking'
}
$beforePublish = $winhttp.Substring(0, $publishMarker)
$publishAndAfter = $winhttp.Substring($publishMarker)
$updatedPublish = $publishAndAfter.Replace('request.raw', 'request.0')
$publishAndAfter = Require-Changed $publishAndAfter $updatedPublish 'publish handle fields'
$winhttp = $beforePublish + $publishAndAfter
Set-Content $winhttpPath $winhttp -NoNewline -Encoding utf8

$mainPath = 'src/main.rs'
$main = Get-Content $mainPath -Raw

$updated = [regex]::Replace(
    $main,
    '(const POPUP_DURATION: Duration = Duration::from_secs\(6\);\r?\n)',
    ('$1' + "`nthread_local! {`n    static POPUP_TIMER: Timer = Timer::default();`n}`n"),
    1
)
$main = Require-Changed $main $updated 'thread-local popup timer'

$updated = [regex]::Replace(
    $main,
    '(?m)^\s*let popup_timer = Rc::new\(Timer::default\(\)\);\r?\n',
    ''
)
$main = Require-Changed $main $updated 'remove shared timer construction'

$updated = [regex]::Replace(
    $main,
    '(?m)^\s*let popup_timer(?:_events)? = Rc::clone\(&popup_timer(?:_events)?\);\r?\n',
    ''
)
$main = Require-Changed $main $updated 'remove shared timer clones'

$updated = $main.Replace(
    'apply_event(&ui_weak, &popup_weak, &popup_timer, event);',
    'apply_event(&ui_weak, &popup_weak, event);'
)
$main = Require-Changed $main $updated 'event application calls'

$updated = [regex]::Replace($main, '(?m)^\s*popup_timer\.stop\(\);\r?\n', '            POPUP_TIMER.with(|timer| timer.stop());' + "`n")
$main = Require-Changed $main $updated 'quit timer stops'

$updated = [regex]::Replace($main, '(?m)^\s*popup_timer: &Timer,\r?\n', '')
$main = Require-Changed $main $updated 'apply_event timer parameter'

$updated = [regex]::Replace($main, '(?m)^\s*popup_timer,\r?\n', '')
$main = Require-Changed $main $updated 'show_popup timer argument'

$oldTimerBlock = '(?ms)    let popup_weak = popup\.as_weak\(\);\r?\n    popup_timer\.start\(TimerMode::SingleShot, POPUP_DURATION, move \|\| \{\r?\n        if let Some\(popup\) = popup_weak\.upgrade\(\) \{\r?\n            let _ = popup\.hide\(\);\r?\n        \}\r?\n    \}\);'
$newTimerBlock = @'
    POPUP_TIMER.with(|timer| {
        let popup_weak = popup.as_weak();
        timer.start(TimerMode::SingleShot, POPUP_DURATION, move || {
            if let Some(popup) = popup_weak.upgrade() {
                let _ = popup.hide();
            }
        });
    });
'@
$updated = [regex]::Replace($main, $oldTimerBlock, $newTimerBlock, 1)
$main = Require-Changed $main $updated 'event-loop popup timer restart'

if ($main.Contains('popup_timer')) {
    throw 'A non-Send popup_timer reference remains'
}
if (-not $main.Contains('POPUP_TIMER')) {
    throw 'Thread-local popup timer was not installed'
}
Set-Content $mainPath $main -NoNewline -Encoding utf8
