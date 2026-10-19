// SPDX-License-Identifier: GPL-2.0-or-later
#include "obs-receiver-ui.h"
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <map>
#include <memory>
#include <mutex>
#include <obs.h>
#include <sstream>
#include <stdexcept>
#include <vector>

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
// clang-format off
#include <windows.h>
#include <windowsx.h>
#include <commctrl.h>
// clang-format on
namespace {
std::map<obs_source_t *, HWND> editors;
constexpr COLORREF bg = RGB(29, 32, 39), panel = RGB(36, 42, 51),
                   accent = RGB(30, 193, 201), text = RGB(214, 223, 230);
struct Editor {
  obs_source_t *source;
  void *context;
  HWND window;
  HWND fields[8]{};
  HFONT font;
  HBRUSH background, surface;
  int observed_delay = 0;
};
LRESULT CALLBACK keyboard(HWND h, UINT message, WPARAM w, LPARAM l, UINT_PTR,
                          DWORD_PTR) {
  if (message == WM_KEYDOWN) {
    const auto parent = GetParent(h);
    if (w == VK_TAB) {
      SetFocus(GetNextDlgTabItem(parent, h, GetKeyState(VK_SHIFT) < 0));
      return 0;
    }
    if (w == VK_ESCAPE) {
      PostMessageW(parent, WM_CLOSE, 0, 0);
      return 0;
    }
    if (w == VK_RETURN) {
      SendMessageW(parent, WM_COMMAND, 10, 0);
      return 0;
    }
  }
  if (message == WM_NCDESTROY)
    RemoveWindowSubclass(h, keyboard, 1);
  return DefSubclassProc(h, message, w, l);
}
std::wstring wide(const std::string &s) {
  int n = MultiByteToWideChar(CP_UTF8, 0, s.c_str(), -1, nullptr, 0);
  std::wstring result(n, 0);
  MultiByteToWideChar(CP_UTF8, 0, s.c_str(), -1, result.data(), n);
  if (!result.empty())
    result.pop_back();
  return result;
}
std::string value(HWND h) {
  int n = GetWindowTextLengthW(h);
  std::wstring w(n + 1, 0);
  GetWindowTextW(h, w.data(), n + 1);
  int bytes = WideCharToMultiByte(CP_UTF8, 0, w.c_str(), -1, nullptr, 0,
                                  nullptr, nullptr);
  std::string s(bytes, 0);
  WideCharToMultiByte(CP_UTF8, 0, w.c_str(), -1, s.data(), bytes, nullptr,
                      nullptr);
  if (!s.empty())
    s.pop_back();
  return s;
}
int number(HWND h) {
  const auto s = value(h);
  size_t used = 0;
  const auto n = std::stoi(s, &used);
  if (used != s.size())
    throw std::invalid_argument("number");
  return n;
}
void fill(Editor *e) {
  auto s = aumux_settings(e->context);
  e->observed_delay = s.delay_ms;
  const std::wstring values[] = {wide(s.bind),
                                 std::to_wstring(s.port),
                                 wide(s.stream),
                                 wide(s.sender),
                                 std::to_wstring(s.buffer_ms),
                                 std::to_wstring(s.delay_ms)};
  for (int i = 0; i < 6; ++i)
    SetWindowTextW(e->fields[i], values[i].c_str());
  SendMessageW(e->fields[6], CB_SETCURSEL, s.mode, 0);
}
HWND control(Editor *e, const wchar_t *kind, const wchar_t *title, int x, int y,
             int w, int h, int id, DWORD extra = 0) {
  auto c = CreateWindowExW(
      0, kind, title, WS_CHILD | WS_VISIBLE | extra, x, y, w, h, e->window,
      reinterpret_cast<HMENU>(INT_PTR(id)), GetModuleHandleW(nullptr), nullptr);
  SendMessageW(c, WM_SETFONT, reinterpret_cast<WPARAM>(e->font), TRUE);
  if (extra & WS_TABSTOP)
    SetWindowSubclass(c, keyboard, 1, 0);
  return c;
}
void line(HDC dc, int x, int y, int w, const std::wstring &s,
          COLORREF color = text) {
  SetTextColor(dc, color);
  SetBkMode(dc, TRANSPARENT);
  RECT r{x, y, x + w, y + 24};
  DrawTextW(dc, s.c_str(), -1, &r, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
}
void meter(HDC dc, int x, int y, int h, float v, float rms) {
  RECT r{x, y, x + 22, y + h};
  auto black = CreateSolidBrush(RGB(10, 15, 21));
  FillRect(dc, &r, black);
  DeleteObject(black);
  auto position = [h](float f) {
    double db = f > 0 ? 20 * std::log10(f) : -72;
    return int(std::clamp((db + 60) / 60, 0.0, 1.0) * h);
  };
  const int level = position(v);
  for (int bottom = 0; bottom < level; bottom += 5) {
    RECT b{x + 2, y + h - bottom - 4, x + 20, y + h - bottom - 1};
    auto brush = CreateSolidBrush(bottom > h * 0.93  ? RGB(233, 91, 72)
                                  : bottom > h * 0.8 ? RGB(222, 182, 81)
                                                     : RGB(65, 174, 118));
    FillRect(dc, &b, brush);
    DeleteObject(brush);
  }
  const int p = position(rms);
  auto pen = CreatePen(PS_SOLID, 2, accent);
  auto old = SelectObject(dc, pen);
  MoveToEx(dc, x - 2, y + h - p, nullptr);
  LineTo(dc, x + 24, y + h - p);
  SelectObject(dc, old);
  DeleteObject(pen);
}
LRESULT CALLBACK procedure(HWND h, UINT message, WPARAM w, LPARAM l) {
  auto *e = reinterpret_cast<Editor *>(GetWindowLongPtrW(h, GWLP_USERDATA));
  if (message == WM_NCCREATE) {
    auto *c = reinterpret_cast<CREATESTRUCTW *>(l);
    e = static_cast<Editor *>(c->lpCreateParams);
    e->window = h;
    SetWindowLongPtrW(h, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(e));
  }
  if (!e)
    return DefWindowProcW(h, message, w, l);
  switch (message) {
  case WM_CREATE: {
    e->background = CreateSolidBrush(bg);
    e->surface = CreateSolidBrush(panel);
    e->font =
        CreateFontW(-15, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY, DEFAULT_PITCH, L"Segoe UI");
    control(e, L"BUTTON", L"Local OBS", 22, 66, 142, 30, 20,
            BS_OWNERDRAW | WS_TABSTOP);
    control(e, L"BUTTON", L"LAN / Tailscale", 174, 66, 160, 30, 21,
            BS_OWNERDRAW | WS_TABSTOP);
    const wchar_t *labels[] = {
        L"Escuchar en IPv4",        L"Puerto UDP",
        L"Identificador del flujo", L"Emisor IPv4 (opcional)",
        L"Búfer de recepción · ms", L"Delay OBS adicional · ms"};
    for (int i = 0; i < 6; ++i) {
      int y = 112 + i * 60;
      control(e, L"STATIC", labels[i], 22, y, 400, 22, 0);
      e->fields[i] = control(e, L"EDIT", L"", 22, y + 23, 394, 28, 100 + i,
                             WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL);
    }
    control(e, L"STATIC", L"Canales", 22, 475, 394, 22, 0);
    e->fields[6] = control(e, L"COMBOBOX", L"", 22, 499, 394, 140, 106,
                           WS_TABSTOP | CBS_DROPDOWNLIST);
    for (auto t : {L"Estéreo", L"Mono", L"Intercambiar L/R"})
      SendMessageW(e->fields[6], CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(t));
    control(e, L"BUTTON", L"Aplicar", 22, 550, 126, 34, 10,
            BS_OWNERDRAW | WS_TABSTOP);
    control(e, L"BUTTON", L"Reiniciar", 158, 550, 126, 34, 11,
            BS_OWNERDRAW | WS_TABSTOP);
    control(e, L"BUTTON", L"Cerrar", 294, 550, 122, 34, 12,
            BS_OWNERDRAW | WS_TABSTOP);
    fill(e);
    SetTimer(h, 1, 100, nullptr);
    return 0;
  }
  case WM_TIMER: {
    const auto actual = aumux_settings(e->context).delay_ms;
    if (GetFocus() != e->fields[5] && actual != e->observed_delay) {
      SetWindowTextW(e->fields[5], std::to_wstring(actual).c_str());
      e->observed_delay = actual;
    }
    RECT r{432, 0, 760, 640};
    InvalidateRect(h, &r, FALSE);
    return 0;
  }
  case WM_COMMAND:
    if (HIWORD(w) != BN_CLICKED)
      return 0;
    if (LOWORD(w) == 20 || LOWORD(w) == 21) {
      bool local = LOWORD(w) == 20;
      SetWindowTextW(e->fields[0], local ? L"127.0.0.1" : L"0.0.0.0");
      SetWindowTextW(e->fields[1], local ? L"17840" : L"17843");
      SetWindowTextW(e->fields[4], local ? L"0" : L"8");
      return 0;
    }
    if (LOWORD(w) == 10) {
      std::string error;
      try {
        auto s = aumux_settings(e->context);
        s.bind = value(e->fields[0]);
        s.port = number(e->fields[1]);
        s.stream = value(e->fields[2]);
        s.sender = value(e->fields[3]);
        s.buffer_ms = number(e->fields[4]);
        s.delay_ms = number(e->fields[5]);
        s.mode = int(SendMessageW(e->fields[6], CB_GETCURSEL, 0, 0));
        if (!aumux_apply(e->context, s, error))
          MessageBoxW(h, wide(error).c_str(), L"AuMux Audio",
                      MB_OK | MB_ICONWARNING);
      } catch (...) {
        MessageBoxW(h, L"Revisa los valores numéricos.", L"AuMux Audio",
                    MB_OK | MB_ICONWARNING);
      }
      return 0;
    }
    if (LOWORD(w) == 11) {
      std::string error;
      if (!aumux_restart(e->context, error))
        MessageBoxW(h, wide(error).c_str(), L"AuMux Audio",
                    MB_OK | MB_ICONWARNING);
      fill(e);
      return 0;
    }
    if (LOWORD(w) == 12) {
      DestroyWindow(h);
      return 0;
    }
    return 0;
  case WM_CTLCOLORSTATIC:
  case WM_CTLCOLOREDIT:
  case WM_CTLCOLORLISTBOX:
    SetTextColor(reinterpret_cast<HDC>(w), text);
    SetBkColor(reinterpret_cast<HDC>(w), panel);
    return reinterpret_cast<LRESULT>(e->surface);
  case WM_DRAWITEM: {
    auto *d = reinterpret_cast<DRAWITEMSTRUCT *>(l);
    auto b = CreateSolidBrush(d->itemState & ODS_SELECTED ? RGB(42, 92, 103)
                                                          : panel);
    FillRect(d->hDC, &d->rcItem, b);
    DeleteObject(b);
    auto p = CreatePen(PS_SOLID, 1, accent);
    auto old = SelectObject(d->hDC, p);
    auto brush = SelectObject(d->hDC, GetStockObject(NULL_BRUSH));
    Rectangle(d->hDC, 0, 0, d->rcItem.right, d->rcItem.bottom);
    SelectObject(d->hDC, old);
    SelectObject(d->hDC, brush);
    DeleteObject(p);
    wchar_t title[80]{};
    GetWindowTextW(d->hwndItem, title, 80);
    SetTextColor(d->hDC, text);
    SetBkMode(d->hDC, TRANSPARENT);
    DrawTextW(d->hDC, title, -1, &d->rcItem,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    if (d->itemState & ODS_FOCUS)
      DrawFocusRect(d->hDC, &d->rcItem);
    return TRUE;
  }
  case WM_ERASEBKGND:
    return 1;
  case WM_PAINT: {
    PAINTSTRUCT paint;
    auto dc = BeginPaint(h, &paint);
    RECT r;
    GetClientRect(h, &r);
    FillRect(dc, &r, e->background);
    SelectObject(dc, e->font);
    line(dc, 22, 16, 700, L"AuMux Audio  ·  RECEPTOR DE RED", accent);
    auto s = aumux_stats(e->context);
    line(dc, 456, 66, 264, wide(s.status), s.connected ? accent : text);
    line(dc, 456, 94, 250, L"PCM float32 · estéreo · 48 kHz");
    line(dc, 476, 132, 50, L"L");
    line(dc, 558, 132, 50, L"R");
    meter(dc, 477, 165, 214, s.peak[0], s.rms[0]);
    meter(dc, 559, 165, 214, s.peak[1], s.rms[1]);
    for (int i = 0; i <= 6; ++i)
      line(dc, 599, 154 + i * 35, 70, std::to_wstring(-i * 10) + L" dB");
    wchar_t t[160];
    swprintf(t, 160, L"Peak L/R: %.1f / %.1f dBFS",
             s.peak[0] > 0 ? 20 * log10(s.peak[0]) : -90,
             s.peak[1] > 0 ? 20 * log10(s.peak[1]) : -90);
    line(dc, 456, 391, 290, t);
    line(dc, 456, 424, 290, L"Emisor: " + wide(s.sender));
    line(dc, 456, 452, 290, L"Paquetes: " + std::to_wstring(s.received));
    line(dc, 456, 480, 290,
         L"Tardíos / duplicados: " + std::to_wstring(s.late) + L" / " +
             std::to_wstring(s.duplicates));
    line(dc, 456, 508, 290,
         L"Muestras faltantes: " + std::to_wstring(s.missing_frames));
    swprintf(t, 160, L"Cola actual: %.2f ms", s.queued_ms);
    line(dc, 456, 536, 290, t);
    line(dc, 22, 600, 710,
         L"Búfer y delay son ajustes distintos. Latencia física total: sin "
         L"medición.");
    EndPaint(h, &paint);
    return 0;
  }
  case WM_DESTROY:
    editors.erase(e->source);
    KillTimer(h, 1);
    DeleteObject(e->font);
    DeleteObject(e->background);
    DeleteObject(e->surface);
    SetWindowLongPtrW(h, GWLP_USERDATA, 0);
    obs_source_release(e->source);
    delete e;
    return 0;
  default:
    return DefWindowProcW(h, message, w, l);
  }
}
} // namespace
void aumux_show_editor(obs_source_t *source, void *context) {
  if (auto found = editors.find(source); found != editors.end()) {
    SetForegroundWindow(found->second);
    return;
  }
  static std::once_flag registered;
  std::call_once(registered, [] {
    WNDCLASSW c{};
    c.lpfnWndProc = procedure;
    c.hInstance = GetModuleHandleW(nullptr);
    c.lpszClassName = L"AuMuxAudioEditor";
    c.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    RegisterClassW(&c);
  });
  auto *e = new Editor{};
  e->source = obs_source_get_ref(source);
  e->context = context;
  auto h = CreateWindowExW(
      WS_EX_APPWINDOW, L"AuMuxAudioEditor", L"AuMux Audio",
      WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, CW_USEDEFAULT,
      CW_USEDEFAULT, 780, 680, nullptr, nullptr, GetModuleHandleW(nullptr), e);
  if (!h) {
    obs_source_release(e->source);
    delete e;
    return;
  }
  editors[source] = h;
  ShowWindow(h, SW_SHOW);
  UpdateWindow(h);
}
void aumux_close_editors() {
  std::vector<HWND> windows;
  for (auto entry : editors)
    windows.push_back(entry.second);
  for (auto h : windows)
    DestroyWindow(h);
}
#else
#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPainter>
#include <QPointer>
#include <QPushButton>
#include <QSpinBox>
#include <QTimer>
#include <QVBoxLayout>
namespace {
std::map<obs_source_t *, QPointer<QDialog>> editors;
class Meters final : public QWidget {
public:
  AumuxStats state;
  explicit Meters(QWidget *parent) : QWidget(parent) {
    setMinimumSize(160, 270);
  }
  void paintEvent(QPaintEvent *) override {
    QPainter p(this);
    p.fillRect(rect(), QColor("#1d2027"));
    for (int c = 0; c < 2; ++c) {
      const int x = 30 + c * 70, h = height() - 65, y = 28;
      p.fillRect(x, y, 24, h, QColor("#0c1016"));
      const double db =
          state.peak[c] > 0 ? 20 * std::log10(state.peak[c]) : -90;
      const int level = int(std::clamp((db + 60) / 60, 0.0, 1.0) * h);
      for (int n = 0; n < level; n += 5)
        p.fillRect(x + 2, y + h - n - 4, 20, 3,
                   QColor(n > h * .93  ? "#e95b48"
                          : n > h * .8 ? "#deb651"
                                       : "#41ae76"));
      p.setPen(QColor("#d6dfe6"));
      const double rms = state.rms[c] > 0 ? 20 * std::log10(state.rms[c]) : -90;
      const int rms_y = y + h - int(std::clamp((rms + 60) / 60, 0.0, 1.0) * h);
      p.setPen(QColor("#1ec1c9"));
      p.drawLine(x - 2, rms_y, x + 26, rms_y);
      p.setPen(QColor("#d6dfe6"));
      p.drawText(x, 18, c ? "R" : "L");
      p.drawText(x - 10, y + h + 24, QString::number(db, 'f', 1));
    }
  }
};
} // namespace
void aumux_show_editor(obs_source_t *source, void *context) {
  if (!QApplication::instance())
    return;
  if (auto found = editors.find(source);
      found != editors.end() && found->second) {
    found->second->raise();
    found->second->activateWindow();
    return;
  }
  auto *retained = obs_source_get_ref(source);
  auto *dialog = new QDialog;
  editors[source] = dialog;
  dialog->setAttribute(Qt::WA_DeleteOnClose);
  dialog->setWindowTitle("AuMux Audio");
  dialog->resize(740, 630);
  QObject::connect(dialog, &QObject::destroyed, [retained] {
    editors.erase(retained);
    obs_source_release(retained);
  });
  dialog->setStyleSheet(
      "QDialog,QWidget{background:#1d2027;color:#d6dfe6;font:13px sans-serif;} "
      "QLineEdit,QSpinBox,QComboBox{background:#242a33;border:1px solid "
      "#485967;padding:7px;} QPushButton{background:#242a33;border:1px solid "
      "#1ec1c9;padding:9px;} QPushButton:hover{background:#2a5c67;} "
      "QLabel#title{color:#1ec1c9;font-size:19px;font-weight:bold;}");
  auto *layout = new QVBoxLayout(dialog);
  auto *title = new QLabel("AuMux Audio · RECEPTOR DE RED");
  title->setObjectName("title");
  layout->addWidget(title);
  auto *presets = new QHBoxLayout;
  auto *local = new QPushButton("Local OBS");
  auto *network = new QPushButton("LAN / Tailscale");
  presets->addWidget(local);
  presets->addWidget(network);
  layout->addLayout(presets);
  auto *row = new QHBoxLayout;
  auto *form = new QFormLayout;
  const auto config = aumux_settings(context);
  auto *bind = new QLineEdit(QString::fromStdString(config.bind));
  bind->setObjectName("bind");
  auto *port = new QSpinBox;
  port->setObjectName("port");
  port->setRange(1024, 65535);
  port->setValue(config.port);
  auto *stream = new QLineEdit(QString::fromStdString(config.stream));
  stream->setObjectName("stream");
  stream->setMaxLength(31);
  auto *sender = new QLineEdit(QString::fromStdString(config.sender));
  auto *buffer = new QSpinBox;
  buffer->setObjectName("buffer");
  buffer->setRange(0, 100);
  buffer->setSuffix(" ms");
  buffer->setValue(config.buffer_ms);
  auto *delay = new QSpinBox;
  delay->setObjectName("delay");
  delay->setRange(-2000, 2000);
  delay->setSuffix(" ms");
  delay->setValue(config.delay_ms);
  auto *mode = new QComboBox;
  mode->addItems({"Estéreo", "Mono", "Intercambiar L/R"});
  mode->setCurrentIndex(config.mode);
  form->addRow("Escuchar en IPv4", bind);
  form->addRow("Puerto UDP", port);
  form->addRow("Identificador", stream);
  form->addRow("Emisor IPv4 (opcional)", sender);
  form->addRow("Búfer de recepción", buffer);
  form->addRow("Delay OBS adicional", delay);
  form->addRow("Canales", mode);
  row->addLayout(form);
  auto *right = new QVBoxLayout;
  auto *status = new QLabel;
  status->setWordWrap(true);
  right->addWidget(status);
  auto *meters = new Meters(dialog);
  right->addWidget(meters);
  row->addLayout(right);
  layout->addLayout(row);
  auto *diagnostics = new QLabel;
  diagnostics->setWordWrap(true);
  layout->addWidget(diagnostics);
  auto *hint = new QLabel("Búfer y delay son ajustes distintos. Latencia "
                          "física total: sin medición.");
  hint->setWordWrap(true);
  layout->addWidget(hint);
  auto *buttons = new QHBoxLayout;
  auto *apply = new QPushButton("Aplicar");
  auto *restart = new QPushButton("Reiniciar receptor");
  auto *close = new QPushButton("Cerrar");
  buttons->addWidget(apply);
  buttons->addWidget(restart);
  buttons->addWidget(close);
  layout->addLayout(buttons);
  QObject::connect(local, &QPushButton::clicked, [=] {
    bind->setText("127.0.0.1");
    port->setValue(17840);
    buffer->setValue(0);
  });
  QObject::connect(network, &QPushButton::clicked, [=] {
    bind->setText("0.0.0.0");
    port->setValue(17843);
    buffer->setValue(8);
  });
  QObject::connect(apply, &QPushButton::clicked, [=] {
    auto s = aumux_settings(context);
    s.bind = bind->text().toStdString();
    s.port = port->value();
    s.stream = stream->text().toStdString();
    s.sender = sender->text().toStdString();
    s.buffer_ms = buffer->value();
    s.delay_ms = delay->value();
    s.mode = mode->currentIndex();
    std::string error;
    if (!aumux_apply(context, s, error))
      QMessageBox::warning(dialog, "AuMux Audio",
                           QString::fromStdString(error));
  });
  QObject::connect(close, &QPushButton::clicked, dialog, &QDialog::close);
  QObject::connect(restart, &QPushButton::clicked, [=] {
    std::string error;
    if (!aumux_restart(context, error))
      QMessageBox::warning(dialog, "AuMux Audio",
                           QString::fromStdString(error));
  });
  auto *timer = new QTimer(dialog);
  auto observed_delay = std::make_shared<int>(config.delay_ms);
  QObject::connect(timer, &QTimer::timeout, [=] {
    const auto actual = aumux_settings(context).delay_ms;
    if (!delay->hasFocus() && actual != *observed_delay) {
      delay->setValue(actual);
      *observed_delay = actual;
    }
    auto s = aumux_stats(context);
    status->setText(QString::fromStdString(s.status) +
                    "\nPCM float32 · 48 kHz");
    meters->state = s;
    meters->update();
    diagnostics->setText(
        QString(
            "Emisor: %1 · Paquetes: %2 · Tardíos: %3 · Duplicados: "
            "%4\nMuestras faltantes: %5 · Cola actual: %6 ms · Colisiones: %7")
            .arg(QString::fromStdString(s.sender))
            .arg(qulonglong(s.received))
            .arg(qulonglong(s.late))
            .arg(qulonglong(s.duplicates))
            .arg(qulonglong(s.missing_frames))
            .arg(s.queued_ms, 0, 'f', 2)
            .arg(qulonglong(s.collisions)));
  });
  timer->start(100);
  dialog->show();
}
void aumux_close_editors() {
  std::vector<QPointer<QDialog>> windows;
  for (auto entry : editors)
    windows.push_back(entry.second);
  for (auto window : windows)
    delete window.data();
}
#endif
