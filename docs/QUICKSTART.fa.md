# شروع سریع Numera CHA-Solver

این ابزار برای OCR محلی و کمک دسترس‌پذیری با بازبینی انسان است؛ حل همهٔ CAPTCHAها یا پذیرش سرویس‌های واقعی را ادعا نمی‌کند. نسخهٔ 0.3.0 اتصال MCP و بسته‌های Tauri برای Windows، Linux و macOS Apple Silicon را هدف می‌گیرد؛ تأیید اجرای نسخهٔ جدید منوط به نتیجهٔ CI روی همان commit است. شواهد موفق 0.2.0 فقط مربوط به نسخهٔ قبلی Windows هستند.

## 1. نصب

بستهٔ تأییدشدهٔ CI مناسب سیستم را دریافت و checksum را بررسی کن؛ [راهنمای بسته‌های بومی](DESKTOP.md). ساخت و تست این انتشار فقط روی GitHub-hosted انجام می‌شود، نه سیستم محلی. مدل‌ها و داده‌های کاربر داخل بسته قرار نمی‌گیرند؛ سرویس یا اجرای خودکار ساخته نمی‌شود.

## 2. دریافت مدل

```powershell
./bin/numera-cha.exe models fetch ppocr-v6-small
```

فایل‌های مدل با SHA-256 کنترل می‌شوند و وارد Git نمی‌شوند.

## 3. اجرای تشخیص

```powershell
./bin/numera-cha.exe recognize --text "12 * 3 = ?"
```

```powershell
./bin/numera-cha.exe doctor --smoke tests/fixtures/ocr/ascii-smoke.png
```

CLI در Windows/Linux پیش‌فرض CUDA و در macOS پیش‌فرض CPU دارد؛ برنامهٔ دسکتاپ با runtime همراه CPU شروع می‌کند، مگر اینکه دستگاه/runtime را صریح انتخاب کرده باشی. اگر CUDA انتخاب می‌کنی، مسیر معتبر محلی CUDA/cuDNN را در `NUMERA_DLL_DIRECTORY` تنظیم کن. نتیجهٔ تست CPU در CI مدرک عملکرد GPU نیست.

## اتصال ایجنت

نام اتصال دقیقاً `Numera-CHA-Solver-MCP` و آرگومان کانفیگ `"args": ["bridge"]` است. پورت پیش‌فرض1583 قابل تغییر است؛ سرور باید از قبل اجرا شود. نمونهٔ کامل Claude Desktop، Codex و Cursor در [راهنمای MCP](MCP.md) و README آمده است. ایجنت پیشنهاد تشخیص می‌دهد؛ اصلاح و تأیید جواب در رابط محلی با شخص است و هیچ جواب یا توکنی به سایت ثالث ارسال نمی‌شود.

## 4. داشبورد

```powershell
./run.ps1
```

آدرس `http://127.0.0.1:8765` را باز کن. مقدار فایل محلی `data/api-token.txt` را در پنجرهٔ Connect وارد کن؛ این مقدار را در چت، Git یا گزارش نفرست.

برای توقف:

```powershell
./scripts/stop.ps1
```

گزارش پوشش در `reports/COVERAGE.json` است. وضعیت blocked یعنی مدل، داده، قرارداد یا مجوز لازم موجود نیست؛ به‌معنای پشتیبانی نیست. لاگ‌های UTF-8 با زمان UTC در `logs` قرار می‌گیرند.
