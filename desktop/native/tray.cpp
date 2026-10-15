// Windows shell integration; all UI work runs outside the audio/control thread.
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <psapi.h>
#include <atomic>
#include <array>
#include <cstdint>
#include <string>
#include <thread>
#include <mutex>
#include <condition_variable>

namespace {
// Preserve the legacy class/mutex so old Monitor Mix and AuMux cannot run two
// independent audio engines by accidentally starting both versions.
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
std::mutex snapshot_mutex;
std::array<std::wstring, 4> snapshot{
    L"IP local: sin dato", L"Latencia: audio detenido",
    L"CPU AuMux: calculando…", L"RAM AuMux: sin dato"
};
std::wstring wide(const char* value) {
    if (!value) return {};
    const int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value, -1, nullptr, 0);
    if (size < 1) return {};
    std::wstring text(static_cast<size_t>(size), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value, -1, text.data(), size);
    text.resize(static_cast<size_t>(size - 1));
    return text;
}
bool add_icon() {
    icon = {}; icon.cbSize = sizeof(icon); icon.hWnd = window; icon.uID = 1;
    icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP; icon.uCallbackMessage = notify_message;
    icon.hIcon = static_cast<HICON>(LoadImageW(GetModuleHandleW(nullptr), MAKEINTRESOURCEW(1), IMAGE_ICON,
        GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_SHARED));
    if (!icon.hIcon) icon.hIcon = LoadIconW(nullptr, MAKEINTRESOURCEW(32512));
    lstrcpynW(icon.szTip, L"AuMux — doble clic para abrir", ARRAYSIZE(icon.szTip));
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
            std::array<std::wstring, 4> current;
            { std::lock_guard<std::mutex> lock(snapshot_mutex); current = snapshot; }
            for (const auto& row : current)
                AppendMenuW(menu, MF_STRING | MF_DISABLED | MF_GRAYED, 0, row.c_str());
            AppendMenuW(menu, MF_STRING, 2, L"Cerrar servicio");
            SetForegroundWindow(hwnd);
            const auto selected = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, nullptr);
            DestroyMenu(menu); PostMessageW(hwnd, WM_NULL, 0, 0);
            if (selected == 1) actions.fetch_or(1);
            if (selected == 2 && MessageBoxW(hwnd,
                    L"¿Seguro que deseas cerrar AuMux?\nSe detendrá el audio y se desconectarán los accesos remotos.",
                    L"Cerrar AuMux", MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND) == IDYES)
                actions.fetch_or(2);
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
        else MessageBoxW(nullptr, L"AuMux ya se está iniciando.", L"AuMux", MB_OK | MB_ICONINFORMATION);
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
        window = CreateWindowExW(0, window_class, L"AuMux", WS_OVERLAPPED, 0, 0, 0, 0, nullptr, nullptr, wc.hInstance, nullptr);
        const bool added = window && add_icon();
        { std::lock_guard<std::mutex> lock(readiness_mutex); succeeded = added; ready = true; }
        readiness.notify_one();
        if (window) { MSG message; while (GetMessageW(&message, nullptr, 0, 0) > 0) { TranslateMessage(&message); DispatchMessageW(&message); } }
    }); } catch (...) { return 0; }
    std::unique_lock<std::mutex> lock(readiness_mutex); readiness.wait(lock, [] { return ready; });
    return succeeded ? 1 : 0;
}
unsigned mm_tray_poll() { return actions.exchange(0); }
void mm_tray_update(const char* ip, const char* latency, const char* cpu, const char* ram) {
    std::array<std::wstring, 4> next{wide(ip), wide(latency), wide(cpu), wide(ram)};
    std::lock_guard<std::mutex> lock(snapshot_mutex); snapshot = std::move(next);
}
unsigned mm_process_metrics(std::uint64_t* cpu_100ns, std::uint64_t* resident_bytes) {
    unsigned available = 0;
    FILETIME created{}, exited{}, kernel{}, user{};
    const auto process = GetCurrentProcess();
    if (GetProcessTimes(process, &created, &exited, &kernel, &user)) {
        const auto ticks = [](FILETIME value) {
            return (static_cast<std::uint64_t>(value.dwHighDateTime) << 32) | value.dwLowDateTime;
        };
        *cpu_100ns = ticks(kernel) + ticks(user); available |= 1;
    }
    PROCESS_MEMORY_COUNTERS counters{};
    counters.cb = sizeof(counters);
    if (GetProcessMemoryInfo(process, &counters, sizeof(counters))) {
        *resident_bytes = static_cast<std::uint64_t>(counters.WorkingSetSize); available |= 2;
    }
    return available;
}
unsigned mm_logical_processors() {
    const auto count = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS);
    return count ? count : 1;
}
void mm_tray_stop() {
    if (window) PostMessageW(window, WM_CLOSE, 0, 0);
    if (worker.joinable()) worker.join(); window = nullptr;
    if (instance_mutex) { CloseHandle(instance_mutex); instance_mutex = nullptr; }
}
void mm_desktop_open_url(const char* url) {
    const auto text = wide(url);
    if (!text.empty()) ShellExecuteW(nullptr, L"open", text.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
}
void mm_desktop_error(const char* error) {
    const auto text = wide(error);
    MessageBoxW(nullptr, text.c_str(), L"AuMux", MB_OK | MB_ICONERROR);
}
}
#endif
