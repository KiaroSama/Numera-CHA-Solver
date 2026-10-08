# شروع سریع Numera CHA-Solver

این ابزار برای OCR محلی و آزمایش کنترل‌شده است؛ حل همهٔ CAPTCHAها یا پذیرش توسط سرویس‌های واقعی را ادعا نمی‌کند. مهاجرت Rust 0.2.0 هنوز در مرحلهٔ اعتبارسنجی است؛ تا پایان آزمون‌های نصب، چرخهٔ پردازش و بازیابی، نسخهٔ قبلی حذف نمی‌شود.

## 1. نصب

PowerShell 7 را در پوشهٔ پروژه باز کن:

```powershell
./scripts/setup.ps1
```

نصب فقط پس از تأیید انجام می‌شود. وابستگی‌ها داخل پروژه می‌مانند؛ سرویس یا اجرای خودکار ویندوز ساخته نمی‌شود.

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

حالت پیش‌فرض CUDA است. اگر DLLهای CUDA/cuDNN پیدا نمی‌شوند، مسیر معتبر محلی را در `NUMERA_DLL_DIRECTORY` تنظیم کن. CPU فقط با انتخاب صریح `--device cpu` اجرا می‌شود.

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
