#include <ApplicationServices/ApplicationServices.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static void key(CGEventSourceRef source, CGKeyCode code, bool down, CGEventFlags flags) {
    CGEventRef event = CGEventCreateKeyboardEvent(source, code, down);
    if (code == 63) CGEventSetType(event, kCGEventFlagsChanged);
    CGEventSetFlags(event, flags);
    CGEventPost(kCGHIDEventTap, event);
    CFRelease(event);
}

int main(int argc, char **argv) {
    if (!AXIsProcessTrusted()) {
        fputs("请在系统设置的辅助功能权限中允许启动服务的终端控制键盘\n", stderr);
        return 1;
    }
    if (argc > 1 && strcmp(argv[1], "--check") == 0) return 0;
    // Fn+Space matches the user's WeType shortcut. AppleScript lacks Fn.
    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateHIDSystemState);
    key(source, 63, true, kCGEventFlagMaskSecondaryFn);
    usleep(20000);
    key(source, 49, true, kCGEventFlagMaskSecondaryFn);
    usleep(40000);
    key(source, 49, false, kCGEventFlagMaskSecondaryFn);
    key(source, 63, false, 0);
    CFRelease(source);
    return 0;
}
