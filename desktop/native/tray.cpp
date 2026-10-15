// Windows shell integration; all UI work runs outside the audio/control thread.
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <atomic>
#include <thread>
#include <mutex>
#include <condition_variable>

namespace {
constexpr wchar_t window_class[] = L"MonitorMix.NotificationWindow";
constexpr UINT notify_message = WM_APP + 1, reopen_message = WM_APP + 2;
std::atomic<unsigned> actions{0};
HANDLE instance_mutex = nullptr;
HWND window = nullptr;
std::thread worker;
std::mutex readiness_mutex;
std::condition_variable readiness;
bool ready = false, succeeded = false;
UINT taskbar_created;
NOTIFYICONDATAW icon{};
bool add_icon() {
    icon = {}; icon.cbSize = sizeof(icon); icon.hWnd = window; icon.uID = 1;
    icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP; icon.uCallbackMessage = notify_message;
    icon.hIcon = LoadIconW(nullptr, MAKEINTRESOURCEW(32512));
    lstrcpynW(icon.szTip, L"Monitor Mix — doble clic para abrir", ARRAYSIZE(icon.szTip));
    return Shell_NotifyIconW(NIM_ADD, &icon) != FALSE;
}
LRESULT CALLBACK procedure(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == taskbar_created) { add_icon(); return 0; }
    if (message == reopen_message) { actions.fetch_or(1); return 0; }
    if (message == notify_message) {
        if (lparam == WM_LBUTTONDBLCLK) actions.fetch_or(1);
        if (lparam == WM_RBUTTONUP || lparam == WM_CONTEXTMENU) {
            POINT point; GetCursorPos(&point);
            HMENU menu = CreatePopupMenu();
            AppendMenuW(menu, MF_STRING, 1, L"Abrir consola");
            AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
            AppendMenuW(menu, MF_STRING, 2, L"Salir (detiene el audio)");
            SetForegroundWindow(hwnd);
            const auto selected = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, nullptr);
            if (selected == 1) actions.fetch_or(1);
            if (selected == 2) actions.fetch_or(2);
            DestroyMenu(menu); PostMessageW(hwnd, WM_NULL, 0, 0);
        }
        return 0;
    }
    if (message == WM_CLOSE) { DestroyWindow(hwnd); return 0; }
    if (message == WM_DESTROY) { Shell_NotifyIconW(NIM_DELETE, &icon); PostQuitMessage(0); return 0; }
    return DefWindowProcW(hwnd, message, wparam, lparam);
}
}
extern "C" {
// Called before audio/device creation. A second normal launch reopens the first.
int mm_tray_claim() {
    instance_mutex = CreateMutexW(nullptr, FALSE, L"Local\\MonitorMix.DesktopInstance");
    if (!instance_mutex) return -1;
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        if (auto existing = FindWindowW(window_class, nullptr)) PostMessageW(existing, reopen_message, 0, 0);
        else MessageBoxW(nullptr, L"Monitor Mix ya se está iniciando.", L"Monitor Mix", MB_OK | MB_ICONINFORMATION);
        CloseHandle(instance_mutex); instance_mutex = nullptr; return 0;
    }
    return 1;
}
int mm_tray_start() {
    ready = false; succeeded = false;
    try { worker = std::thread([] {
        WNDCLASSW wc{}; wc.lpfnWndProc = procedure; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = window_class;
        taskbar_created = RegisterWindowMessageW(L"TaskbarCreated");
        RegisterClassW(&wc);
        window = CreateWindowExW(0, window_class, L"Monitor Mix", WS_OVERLAPPED, 0, 0, 0, 0, nullptr, nullptr, wc.hInstance, nullptr);
        const bool added = window && add_icon();
        { std::lock_guard<std::mutex> lock(readiness_mutex); succeeded = added; ready = true; }
        readiness.notify_one();
        if (window) { MSG message; while (GetMessageW(&message, nullptr, 0, 0) > 0) { TranslateMessage(&message); DispatchMessageW(&message); } }
    }); } catch (...) { return 0; }
    std::unique_lock<std::mutex> lock(readiness_mutex); readiness.wait(lock, [] { return ready; });
    return succeeded ? 1 : 0;
}
unsigned mm_tray_poll() { return actions.exchange(0); }
void mm_tray_stop() {
    if (window) PostMessageW(window, WM_CLOSE, 0, 0);
    if (worker.joinable()) worker.join(); window = nullptr;
    if (instance_mutex) { CloseHandle(instance_mutex); instance_mutex = nullptr; }
}
void mm_desktop_open_url(const char* url) {
    const int size = MultiByteToWideChar(CP_UTF8, 0, url, -1, nullptr, 0);
    auto text = new wchar_t[size]; MultiByteToWideChar(CP_UTF8, 0, url, -1, text, size);
    ShellExecuteW(nullptr, L"open", text, nullptr, nullptr, SW_SHOWNORMAL); delete[] text;
}
void mm_desktop_error(const char* error) {
    const int size = MultiByteToWideChar(CP_UTF8, 0, error, -1, nullptr, 0);
    auto text = new wchar_t[size]; MultiByteToWideChar(CP_UTF8, 0, error, -1, text, size);
    MessageBoxW(nullptr, text, L"Monitor Mix", MB_OK | MB_ICONERROR); delete[] text;
}
}
#endif
