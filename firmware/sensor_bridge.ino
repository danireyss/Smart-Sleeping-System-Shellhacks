// Sends sensor readings from the UNO Q microcontroller to the Linux side
// through the Arduino router. Sketch libraries required in App Lab:
// "DHT sensor library" (Adafruit), "Adafruit Unified Sensor", "Adafruit CCS811 Library".
//
// Bridge.notify("reading", ...) params, in order (parsed by backend/src/adapters/bridge.rs):
//   0. eco2      int    eCO2 ppm (estimated by the CCS811)
//   1. tvoc      int    TVOC ppb
//   2. temp_f    float  degrees F, NaN if the DHT11 read failed
//   3. humidity  float  %RH, NaN if the DHT11 read failed
//   4. uptime_s  int    seconds since the sketch started (backend flags < 1200 as CCS811 warm-up)
#include <Arduino_RouterBridge.h>
#include <Adafruit_CCS811.h>
#include <DHT.h>

#define DHT_POWER 7
#define DHT_DATA  2
#define SEND_INTERVAL_MS 10000   // 60000 in production

DHT dht(DHT_DATA, DHT11);
Adafruit_CCS811 ccs;
uint16_t eco2 = 0, tvoc = 0;
unsigned long lastSend = 0;

void setup() {
  pinMode(DHT_POWER, OUTPUT);
  digitalWrite(DHT_POWER, HIGH);   // power the DHT11 from pin 7
  Bridge.begin();
  Monitor.begin();
  delay(2000);
  dht.begin();
  if (!ccs.begin()) Monitor.println("CCS811 failed to start");
}

void loop() {
  if (ccs.available() && !ccs.readData()) {   // keep latest CCS values
    eco2 = ccs.geteCO2();
    tvoc = ccs.getTVOC();
  }
  if (millis() - lastSend >= SEND_INTERVAL_MS) {
    lastSend = millis();
    float h = dht.readHumidity();
    float c = dht.readTemperature();
    if (!isnan(h) && !isnan(c)) ccs.setEnvironmentalData(h, c);
    float f = isnan(c) ? NAN : c * 9.0 / 5.0 + 32;
    int uptime_s = millis() / 1000;
    Bridge.notify("reading", (int)eco2, (int)tvoc, f, h, uptime_s);
    Monitor.print("Sent: "); Monitor.print(eco2); Monitor.print(" ppm, ");
    Monitor.print(f); Monitor.print(" F, "); Monitor.print(h); Monitor.println(" %");
  }
  delay(1000);
}
