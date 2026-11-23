#pragma once
#include <QtGui/QGuiApplication>

namespace murk {
// Taskbar overlay on Windows, dock badge on macOS; zero clears it.
inline void set_badge_number(int n) {
    if (qGuiApp)
        qGuiApp->setBadgeNumber(n);
}
}
