// Hypnos device sketch (UNO Q MCU): reads the sensors, sends readings to
// the Linux backend through the Arduino router, and drives the 3.5" 480x320
// SPI touch screen with the live score and the Sleep button.
//
// Sketch libraries required in App Lab: "DHT sensor library" (Adafruit),
// "Adafruit Unified Sensor", "Adafruit CCS811 Library", "Adafruit GFX Library".
//
// Wiring
//   Breadboard rails: UNO Q 3.3V -> + rail, GND -> - rail
//   DHT11   V -> + rail, G -> - rail, S -> pin 2
//   CCS811  VCC -> + rail, GND and WAKE -> - rail, SDA/SCL -> SDA/SCL
//   Screen  LCD CS 10, DC 9, RST 8, touch CS 7, SPI on 11/12/13
//   Backlight: set PIN_BACKLIGHT to a PWM pin (3, 5, 6...) only if the module has
//   a BL/LED pin wired to it; -1 means the backlight is always on.
//
// Backend protocol (backend/src/adapters/bridge.rs, backend/src/domain/device.rs)
//   Bridge.notify("reading", eco2, tvoc, temp_f, humidity, uptime_s)
//     0. eco2      int    eCO2 ppm (estimated by the CCS811)
//     1. tvoc      int    TVOC ppb
//     2. temp_f    float  degrees F, NaN if the DHT11 read failed
//     3. humidity  float  %RH, NaN if the DHT11 read failed
//     4. uptime_s  int    seconds since the sketch started (< 1200 = CCS811 warm-up)
//   Bridge.call("sleep_start") -> true    tap on the Sleep button
//   Bridge.call("sleep_end")   -> false   1.5 s hold during sleep mode
//   Bridge.call("sleep_state") -> bool    polled, so the screen follows the web/curl
//   Bridge.call("score")       -> float   latest total score, -1 when there is none
#include <Arduino_RouterBridge.h>
#include <Adafruit_CCS811.h>
#include <Adafruit_GFX.h>
#include <DHT.h>
#include <SPI.h>

// ---------- Pins and timing ----------
#define DHT_DATA  2
// The DHT11 is powered from the 3.3V rail. If it's ever wired to a GPIO for power
// instead (no breadboard), set that pin here; -1 means rail-powered.
const int DHT_POWER_PIN = -1;
const int PIN_TOUCH_CS  = 7;
const int PIN_RST       = 8;
const int PIN_DC        = 9;
const int PIN_LCD_CS    = 10;
#define PIN_BACKLIGHT -1    // -1: no software backlight control (compiled out)

const unsigned long SEND_INTERVAL_MS    = 60000;  // readings to the backend (10000 for testing)
const unsigned long DISPLAY_INTERVAL_MS = 10000;  // sensor values on screen
const unsigned long SYNC_INTERVAL_MS    = 10000;  // score + sleep state from the backend
const unsigned long HOLD_TO_END_MS      = 1500;
const unsigned long WARM_UP_S           = 1200;   // CCS811 warm-up (matches the backend)

// Not WIDTH/HEIGHT: Adafruit_GFX has members with those names, and inside the Lcd
// class they would shadow these constants (GFX then thinks the screen is 0x0).
const int SCREEN_W = 480;
const int SCREEN_H = 320;

// Touch calibration (from the screen test)
const bool SWAP_XY = true;
const bool FLIP_X  = true;
const bool FLIP_Y  = false;

// ---------- Colors (RGB565, from docs/DESIGN.md) ----------
const uint16_t C_BG      = 0x0000;  // black
const uint16_t C_SURFACE = 0x10A3;  // #14171c
const uint16_t C_TEXT    = 0xCE7A;  // #c9cdd3
const uint16_t C_MUTED   = 0x7C11;  // #7a808a
const uint16_t C_DIM     = 0x2104;  // barely visible, for sleep mode
const uint16_t C_GREAT   = 0x4D95;  // teal   #4fb3a9
const uint16_t C_GOOD    = 0x7DAF;  // green  #7fb77e
const uint16_t C_FAIR    = 0xDD28;  // amber  #d9a441
const uint16_t C_POOR    = 0xE3CD;  // coral  #e07a6a

// ---------- Screen: low-level SPI (from the working test sketch) ----------
void writeWord(uint16_t w) {
  digitalWrite(PIN_LCD_CS, LOW);
  SPI.transfer(w >> 8);
  SPI.transfer(w & 0xFF);
  digitalWrite(PIN_LCD_CS, HIGH);
}

void writeCommand(uint8_t cmd) {
  digitalWrite(PIN_DC, LOW);
  writeWord(cmd);
}

void writeData(uint8_t data) {
  digitalWrite(PIN_DC, HIGH);
  writeWord(data);
}

void setWindow(int x0, int y0, int x1, int y1) {
  writeCommand(0x2A);
  writeData(x0 >> 8); writeData(x0 & 0xFF);
  writeData(x1 >> 8); writeData(x1 & 0xFF);
  writeCommand(0x2B);
  writeData(y0 >> 8); writeData(y0 & 0xFF);
  writeData(y1 >> 8); writeData(y1 & 0xFF);
  writeCommand(0x2C);
}

void lcdFill(int x, int y, int w, int h, uint16_t color) {
  if (x < 0) { w += x; x = 0; }
  if (y < 0) { h += y; y = 0; }
  if (x + w > SCREEN_W)  w = SCREEN_W - x;
  if (y + h > SCREEN_H) h = SCREEN_H - y;
  if (w <= 0 || h <= 0) return;
  setWindow(x, y, x + w - 1, y + h - 1);
  digitalWrite(PIN_DC, HIGH);
  for (long i = 0; i < (long)w * h; i++) {
    writeWord(color);
  }
}

// Adafruit GFX on top of lcdFill, for text and shapes.
class Lcd : public Adafruit_GFX {
 public:
  Lcd() : Adafruit_GFX(SCREEN_W, SCREEN_H) {}
  void drawPixel(int16_t x, int16_t y, uint16_t c) override { lcdFill(x, y, 1, 1, c); }
  void writePixel(int16_t x, int16_t y, uint16_t c) override { lcdFill(x, y, 1, 1, c); }
  void fillRect(int16_t x, int16_t y, int16_t w, int16_t h, uint16_t c) override { lcdFill(x, y, w, h, c); }
  void writeFillRect(int16_t x, int16_t y, int16_t w, int16_t h, uint16_t c) override { lcdFill(x, y, w, h, c); }
  void drawFastHLine(int16_t x, int16_t y, int16_t w, uint16_t c) override { lcdFill(x, y, w, 1, c); }
  void drawFastVLine(int16_t x, int16_t y, int16_t h, uint16_t c) override { lcdFill(x, y, 1, h, c); }
  void writeFastHLine(int16_t x, int16_t y, int16_t w, uint16_t c) override { lcdFill(x, y, w, 1, c); }
  void writeFastVLine(int16_t x, int16_t y, int16_t h, uint16_t c) override { lcdFill(x, y, 1, h, c); }
  void fillScreen(uint16_t c) override { lcdFill(0, 0, SCREEN_W, SCREEN_H, c); }
};
Lcd tft;

// ---------- Touch (XPT2046, from the working test sketch) ----------
uint16_t readTouchChannel(uint8_t cmd) {
  SPI.transfer(cmd);
  uint16_t hi = SPI.transfer(0);
  uint16_t lo = SPI.transfer(0);
  return ((hi << 8) | lo) >> 3;  // 12-bit value
}

bool getTouch(int &x, int &y) {
  // Touch chip needs a slower speed than the screen
  SPI.endTransaction();
  SPI.beginTransaction(SPISettings(1000000, MSBFIRST, SPI_MODE0));
  digitalWrite(PIN_TOUCH_CS, LOW);

  uint16_t pressure = readTouchChannel(0xB0);
  long sumX = 0, sumY = 0;
  for (int i = 0; i < 4; i++) {
    sumX += readTouchChannel(0xD0);
    sumY += readTouchChannel(0x90);
  }

  digitalWrite(PIN_TOUCH_CS, HIGH);
  SPI.endTransaction();
  SPI.beginTransaction(SPISettings(8000000, MSBFIRST, SPI_MODE0));

  if (pressure < 100) return false;  // not being touched

  int rawX = sumX / 4;
  int rawY = sumY / 4;
  if (SWAP_XY) { int t = rawX; rawX = rawY; rawY = t; }

  x = map(rawX, 200, 3900, 0, SCREEN_W - 1);
  y = map(rawY, 200, 3900, 0, SCREEN_H - 1);
  if (FLIP_X) x = SCREEN_W - 1 - x;
  if (FLIP_Y) y = SCREEN_H - 1 - y;
  x = constrain(x, 0, SCREEN_W - 1);
  y = constrain(y, 0, SCREEN_H - 1);
  return true;
}

// ---------- State ----------
DHT dht(DHT_DATA, DHT11);
Adafruit_CCS811 ccs;
uint16_t eco2 = 0, tvoc = 0;
float tempF = NAN, humidity = NAN;
float score = -1;          // from the backend; -1 = none
bool backendOk = false;    // last Bridge.call succeeded
bool sleeping = false;

unsigned long lastSend = 0, lastDisplay = 0, lastSync = 0;
bool touching = false;           // finger currently down
unsigned long touchStart = 0;    // when the current touch began
bool touchHandled = false;       // this touch already triggered an action
int touchX = 0, touchY = 0;      // last position while touching (release has none)

// Button: bottom of the screen
const int BTN_X = 16, BTN_Y = 240, BTN_W = SCREEN_W - 32, BTN_H = 64;
bool inButton(int x, int y) {
  return x >= BTN_X && x < BTN_X + BTN_W && y >= BTN_Y && y < BTN_Y + BTN_H;
}

// Defined further down; declared here so the sketch doesn't rely on the IDE
// generating prototypes.
void drawAll();

// ---------- Backend calls ----------
void syncWithBackend() {
  bool state;
  float s;
  bool okState = Bridge.call("sleep_state").result(state);
  bool okScore = Bridge.call("score").result(s);
  backendOk = okState && okScore;
  if (okScore) score = s;
  if (okState && state != sleeping) {
    sleeping = state;  // started or ended from the web / curl
    drawAll();
  }
}

void startSleep() {
  bool state;
  if (Bridge.call("sleep_start").result(state)) {
    sleeping = state;
    backendOk = true;
  } else {
    backendOk = false;
  }
  drawAll();
}

void endSleep() {
  bool state;
  if (Bridge.call("sleep_end").result(state)) {
    sleeping = state;
    backendOk = true;
  } else {
    backendOk = false;
  }
  drawAll();
}

// ---------- Drawing ----------
uint16_t bandColor(float s) {
  if (s >= 90) return C_GREAT;
  if (s >= 80) return C_GOOD;
  if (s >= 70) return C_FAIR;
  return C_POOR;
}

const char *bandName(float s) {
  if (s >= 90) return "Great";
  if (s >= 80) return "Good";
  if (s >= 70) return "Fair";
  return "Poor";
}

void text(int x, int y, uint8_t size, uint16_t color, uint16_t bg) {
  tft.setTextSize(size);
  tft.setTextColor(color, bg);
  tft.setCursor(x, y);
}

void setBacklight(bool on) {
#if PIN_BACKLIGHT >= 0
  analogWrite(PIN_BACKLIGHT, on ? 255 : 0);
#else
  (void)on;
#endif
}

void drawScore() {
  lcdFill(16, 40, 448, 70, C_BG);
  bool warming = millis() / 1000 < WARM_UP_S;
  if (score < 0) {
    text(16, 44, 7, C_TEXT, C_BG);
    tft.print("--");
    text(150, 62, 2, C_MUTED, C_BG);
    tft.print(!backendOk ? "Hub offline" : warming ? "Sensor warming up" : "No score yet");
    return;
  }
  text(16, 44, 7, C_TEXT, C_BG);
  tft.print(score, 1);
  int x = tft.getCursorX() + 20;
  lcdFill(x, 62, 12, 12, bandColor(score));  // band dot
  text(x + 22, 58, 3, bandColor(score), C_BG);
  tft.print(bandName(score));
}

void drawMetric(int x, const char *label, float value, int decimals, const char *unit) {
  lcdFill(x, 140, 150, 80, C_BG);
  text(x, 140, 2, C_MUTED, C_BG);
  tft.print(label);
  text(x, 170, 3, C_TEXT, C_BG);
  if (isnan(value)) tft.print("--");
  else tft.print(value, decimals);
  text(x, 200, 2, C_MUTED, C_BG);
  tft.print(unit);
}

void drawReadings() {
  drawMetric(16, "eCO2 est.", eco2 == 0 ? NAN : (float)eco2, 0, "ppm");
  drawMetric(176, "Temp", tempF, 1, "F");
  drawMetric(336, "Humidity", humidity, 1, "%");
}

void drawButton(uint16_t fill, uint16_t color, const char *label) {
  lcdFill(BTN_X, BTN_Y, BTN_W, BTN_H, fill);
  int16_t bx, by;
  uint16_t bw, bh;
  tft.setTextSize(3);
  tft.getTextBounds(label, 0, 0, &bx, &by, &bw, &bh);
  text(BTN_X + (BTN_W - bw) / 2, BTN_Y + (BTN_H - bh) / 2, 3, color, fill);
  tft.print(label);
}

void drawAll() {
  tft.fillScreen(C_BG);
  if (sleeping) {
    // As dark as possible: black screen, one barely visible hint.
    setBacklight(false);
    lcdFill(BTN_X, BTN_Y, BTN_W, BTN_H, C_BG);
    text(BTN_X + 92, BTN_Y + 26, 2, C_DIM, C_BG);
    tft.print("Hold here to end sleep");
    return;
  }
  setBacklight(true);
  text(16, 12, 2, C_MUTED, C_BG);
  tft.print("HYPNOS");
  drawScore();
  drawReadings();
  drawButton(C_GREAT, C_BG, "Sleep");
}

// Progress bar while holding to end sleep mode (dim, no animation otherwise).
void drawHoldProgress(unsigned long heldMs) {
  int w = (long)BTN_W * min(heldMs, HOLD_TO_END_MS) / HOLD_TO_END_MS;
  lcdFill(BTN_X, BTN_Y + BTN_H - 4, w, 4, C_DIM);
}

// ---------- Main ----------
void readSensors() {
  float h = dht.readHumidity();
  float c = dht.readTemperature();
  if (!isnan(h) && !isnan(c)) ccs.setEnvironmentalData(h, c);
  humidity = h;
  tempF = isnan(c) ? NAN : c * 9.0 / 5.0 + 32;
}

void handleTouch() {
  int x, y;
  bool down = getTouch(x, y);
  unsigned long now = millis();

  if (down) {
    touchX = x;
    touchY = y;
  }
  if (down && !touching) {           // touch began
    touching = true;
    touchStart = now;
    touchHandled = false;
  }
  if (down && sleeping && !touchHandled && inButton(touchX, touchY)) {
    unsigned long held = now - touchStart;
    drawHoldProgress(held);
    if (held >= HOLD_TO_END_MS) {
      touchHandled = true;
      endSleep();
    }
  }
  if (!down && touching) {           // touch released
    touching = false;
    if (sleeping && !touchHandled) {
      lcdFill(BTN_X, BTN_Y + BTN_H - 4, BTN_W, 4, C_BG);  // clear the progress bar
    }
    // Start on release (a tap), so resting a hand on the screen doesn't trigger it.
    if (!sleeping && !touchHandled && inButton(touchX, touchY)) {
      touchHandled = true;
      startSleep();
    }
  }
}

void setup() {
  // 1. Screen first, exactly as in the working screen test, so it shows something
  //    even if a later step stalls.
  pinMode(PIN_TOUCH_CS, OUTPUT);
  pinMode(PIN_RST, OUTPUT);
  pinMode(PIN_DC, OUTPUT);
  pinMode(PIN_LCD_CS, OUTPUT);
  digitalWrite(PIN_TOUCH_CS, HIGH);
  digitalWrite(PIN_LCD_CS, HIGH);

  SPI.begin();
  SPI.beginTransaction(SPISettings(8000000, MSBFIRST, SPI_MODE0));
  digitalWrite(PIN_RST, HIGH); delay(50);
  digitalWrite(PIN_RST, LOW);  delay(50);
  digitalWrite(PIN_RST, HIGH); delay(150);
  writeCommand(0x01); delay(150);  // reset
  writeCommand(0x11); delay(150);  // wake
  writeCommand(0x3A); writeData(0x55);  // 16-bit color
  writeCommand(0x36); writeData(0x28);  // landscape
  writeCommand(0x29); delay(50);        // display on

  // Boot check: black screen + teal bar (direct drawing), then text (Adafruit GFX).
  //   still the old picture  -> this sketch isn't running
  //   black + bar, no text   -> Adafruit GFX problem
  lcdFill(0, 0, SCREEN_W, SCREEN_H, C_BG);
  lcdFill(0, 0, SCREEN_W, 6, C_GREAT);
  text(16, 150, 2, C_MUTED, C_BG);
  tft.print("Starting...");

  // 2. Bridge and serial monitor.
  Bridge.begin();
  Monitor.begin();
  Monitor.println("[boot] screen ok, bridge started");

  // 3. Sensors.
  if (DHT_POWER_PIN >= 0) {
    pinMode(DHT_POWER_PIN, OUTPUT);
    digitalWrite(DHT_POWER_PIN, HIGH);
  }
#if PIN_BACKLIGHT >= 0
  pinMode(PIN_BACKLIGHT, OUTPUT);
#endif
  delay(2000);
  dht.begin();
  if (!ccs.begin()) Monitor.println("[boot] CCS811 failed to start");
  readSensors();
  Monitor.println("[boot] sensors ok");

  // 4. Backend, then the real screen.
  syncWithBackend();
  Monitor.println(backendOk ? "[boot] backend ok" : "[boot] backend not answering");
  drawAll();
  Monitor.println("[boot] done");
}

void loop() {
  unsigned long now = millis();

  if (ccs.available() && !ccs.readData()) {   // keep latest CCS values
    eco2 = ccs.geteCO2();
    tvoc = ccs.getTVOC();
  }

  handleTouch();

  if (now - lastDisplay >= DISPLAY_INTERVAL_MS) {
    lastDisplay = now;
    readSensors();
    if (!sleeping) drawReadings();
  }

  if (now - lastSend >= SEND_INTERVAL_MS) {
    lastSend = now;
    int uptime_s = millis() / 1000;
    Bridge.notify("reading", (int)eco2, (int)tvoc, tempF, humidity, uptime_s);
    Monitor.print("Sent: "); Monitor.print(eco2); Monitor.print(" ppm, ");
    Monitor.print(tempF); Monitor.print(" F, "); Monitor.print(humidity); Monitor.println(" %");
  }

  if (!touching && now - lastSync >= SYNC_INTERVAL_MS) {
    lastSync = now;
    syncWithBackend();
    if (!sleeping) drawScore();
  }

  delay(20);  // ~50 touch polls per second
}
