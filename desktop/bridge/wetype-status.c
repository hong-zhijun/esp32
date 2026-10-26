#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// Observe only whether WeType is capturing audio. No keyboard or audio capture.
static int status(void) {
    AudioObjectPropertyAddress list = {kAudioHardwarePropertyProcessObjectList,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &list, 0, NULL, &size)) return -1;
    AudioObjectID *ids = malloc(size ? size : 1);
    if (!ids) return -1;
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &list, 0, NULL, &size, ids)) { free(ids); return -1; }
    int active = 0;
    for (unsigned i = 0; i < size / sizeof(*ids); i++) {
        AudioObjectPropertyAddress prop = {kAudioProcessPropertyBundleID,
            kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
        CFStringRef bundle = NULL; UInt32 n = sizeof(bundle);
        if (AudioObjectGetPropertyData(ids[i], &prop, 0, NULL, &n, &bundle) || !bundle) continue;
        bool match = CFStringHasPrefix(bundle, CFSTR("com.tencent.inputmethod.wetype"));
        CFRelease(bundle);
        if (!match) continue;
        prop.mSelector = kAudioProcessPropertyIsRunningInput;
        UInt32 running = 0; n = sizeof(running);
        if (AudioObjectGetPropertyData(ids[i], &prop, 0, NULL, &n, &running)) { active = -1; break; }
        if (running) { active = 1; break; }
    }
    free(ids);
    return active;
}

int main(int argc, char **argv) {
    bool watch = argc > 1 && strcmp(argv[1], "--watch") == 0;
    int previous = -2;
    do {
        int current = status();
        if (current != previous) {
            printf("{\"active\":%s,\"available\":%s}\n", current == 1 ? "true" : "false", current >= 0 ? "true" : "false");
            fflush(stdout); previous = current;
        }
        if (!watch) return current < 0 ? 1 : 0;
        usleep(100000);
    } while (1);
}
