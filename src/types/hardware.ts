/**
 * 硬件配置相关类型定义
 */

export type ConnectionMode = 'jtag' | 'uart' | 'unknown';

export interface ConnectionInfo {
  mode: ConnectionMode;
  modeLabel: string;
  recommended: boolean;
  port: string | null;
  vid: string | null;
  pid: string | null;
  chipHint: string | null;
  /** IDF 目标名（如 "esp32s3"），用于自动选择芯片 */
  idfTarget: string | null;
  capabilities: string[];
  recommendation: string;
}

// ESP-IDF 环境信息
export interface IDFEnvironment {
  idf_path: string;
  version: string;
  tools_path: string;
  python_path?: string;
  source: DetectionSource;
}

export type DetectionSource = 'AutoDetected' | 'UserConfigured' | 'EnvVariable' | 'Unknown';

// 外设实例
export interface PeripheralInstance {
  id: string;
  definition_id: string;
  name: string;
  pin_values: Record<string, number>;
  library_choice: string;
  notes: string;
  params?: Record<string, unknown>;
}

// 外设更新请求
export interface PeripheralUpdate {
  name?: string;
  pin_values?: Record<string, number>;
  library_choice?: string;
  notes?: string;
  params?: Record<string, unknown>;
}

// 硬件配置
export interface HardwareConfig {
  project_name: string;
  board: string;
  peripherals: Record<string, PeripheralInstance>;
}

// 引脚冲突
export interface PinConflict {
  pin: number;
  peripheral_a: string;
  peripheral_b: string;
}

// 外设定义（预置外设清单）
export interface PeripheralDefinition {
  id: string;
  name: string;
  category: 'sensor' | 'display' | 'actuator' | 'communication' | 'storage' | 'camera' | 'other';
  required_pins: PinDefinition[];
  optional_pins: PinDefinition[];
  libraries: LibraryOption[];
}

export interface PinDefinition {
  name: string;
  display_name: string;
  required: boolean;
  description?: string;
}

export interface LibraryOption {
  id: string;
  name: string;
  params?: ParamDefinition[];
}

export interface ParamDefinition {
  name: string;
  display_name: string;
  type: 'number' | 'string' | 'boolean' | 'select';
  default?: unknown;
  options?: string[];
  description?: string;
}

// 预置外设数据
export const PRESET_PERIPHERALS: PeripheralDefinition[] = [
  // ==================== 执行器 ====================
  {
    id: 'led',
    name: 'LED',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: 'LED 控制引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'none', name: '无（直接控制）' },
    ],
  },
  {
    id: 'rgb_led',
    name: 'RGB LED',
    category: 'actuator',
    required_pins: [
      { name: 'r', display_name: 'R', required: true, description: '红色引脚' },
      { name: 'g', display_name: 'G', required: true, description: '绿色引脚' },
      { name: 'b', display_name: 'B', required: true, description: '蓝色引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'none', name: '无（PWM控制）' },
      { id: 'esp_led_strip', name: 'ESP LED Strip' },
    ],
  },
  {
    id: 'relay',
    name: '继电器模块',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '控制信号引脚' },
    ],
    optional_pins: [
      { name: 'gpio2', display_name: 'GPIO2', required: false, description: '第二路控制引脚' },
    ],
    libraries: [{ id: 'none', name: '无（GPIO 输出）' }],
  },
  {
    id: 'servo',
    name: '舵机 (SG90/MG995)',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: 'PWM 控制引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'esp32_pwm', name: 'ESP32 MCPWM' },
      { id: 'ledc', name: 'ESP32 LEDC' },
    ],
  },
  {
    id: 'stepper_28byj48',
    name: '步进电机 28BYJ-48',
    category: 'actuator',
    required_pins: [
      { name: 'in1', display_name: 'IN1', required: true, description: '输入1' },
      { name: 'in2', display_name: 'IN2', required: true, description: '输入2' },
      { name: 'in3', display_name: 'IN3', required: true, description: '输入3' },
      { name: 'in4', display_name: 'IN4', required: true, description: '输入4' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（直接控制）' }],
  },
  {
    id: 'dc_motor_l298n',
    name: '直流电机 L298N',
    category: 'actuator',
    required_pins: [
      { name: 'en', display_name: 'EN', required: true, description: '使能/PWM' },
      { name: 'in1', display_name: 'IN1', required: true, description: '方向1' },
      { name: 'in2', display_name: 'IN2', required: true, description: '方向2' },
    ],
    optional_pins: [
      { name: 'en2', display_name: 'EN2', required: false, description: '第二路使能' },
      { name: 'in3', display_name: 'IN3', required: false, description: '第二路方向1' },
      { name: 'in4', display_name: 'IN4', required: false, description: '第二路方向2' },
    ],
    libraries: [{ id: 'none', name: '无（直接控制）' }],
  },
  {
    id: 'buzzer',
    name: '蜂鸣器（有源/无源）',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '控制引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无' }],
  },

  // ==================== 传感器 ====================
  {
    id: 'button',
    name: '按键/开关',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '输入引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO 输入）' }],
  },
  {
    id: 'dht11',
    name: 'DHT11 温湿度传感器',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '数据引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'dht_sensor_lib', name: 'DHT sensor library' }],
  },
  {
    id: 'dht22',
    name: 'DHT22 温湿度传感器',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '数据引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'dht_sensor_lib', name: 'DHT sensor library' }],
  },
  {
    id: 'ds18b20',
    name: 'DS18B20 数字温度',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '1-Wire 数据引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'ds18b20', name: 'DS18B20 OneWire' },
      { id: 'dallas_temp', name: 'Dallas Temperature' },
    ],
  },
  {
    id: 'bmp280',
    name: 'BMP280 气压传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'adafruit_bmp280', name: 'Adafruit BMP280' },
      { id: 'bmp280_esp', name: 'ESP BMP280 Component' },
    ],
  },
  {
    id: 'bme280',
    name: 'BME280 温湿压传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'bme280', name: 'BME280 Driver' },
      { id: 'adafruit_bme280', name: 'Adafruit BME280' },
    ],
  },
  {
    id: 'aht10',
    name: 'AHT10/AHT20 温湿度',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'aht10_esp', name: 'AHT10 ESP Component' },
    ],
  },
  {
    id: 'mpu6050',
    name: 'MPU6050 六轴传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'mpu6050_esp', name: 'MPU6050 Driver' },
    ],
  },
  {
    id: 'hc_sr04',
    name: 'HC-SR04 超声波测距',
    category: 'sensor',
    required_pins: [
      { name: 'trig', display_name: 'TRIG', required: true, description: '触发引脚' },
      { name: 'echo', display_name: 'ECHO', required: true, description: '回响引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'ultrasonic', name: 'Ultrasonic Library' }],
  },
  {
    id: 'bh1750',
    name: 'BH1750 光照传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'bh1750_esp', name: 'BH1750 Driver' },
    ],
  },
  {
    id: 'pir_hcsr501',
    name: 'PIR 人体红外 (HC-SR501)',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '输出信号引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO 输入）' }],
  },
  {
    id: 'mq2',
    name: 'MQ-2 烟雾/可燃气体',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'mq135',
    name: 'MQ-135 空气质量传感器',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'soil_moisture',
    name: '土壤湿度传感器',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'touch_ttp223',
    name: '触摸传感器 TTP223',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '输出信号引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO 输入）' }],
  },
  {
    id: 'rc522_rfid',
    name: 'RC522 RFID 模块',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA/SS', required: true, description: 'SPI 片选' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'miso', display_name: 'MISO', required: true, description: 'SPI 主入从出' },
    ],
    optional_pins: [
      { name: 'rst', display_name: 'RST', required: false, description: '复位引脚' },
    ],
    libraries: [{ id: 'mfrc522', name: 'MFRC522 Library' }],
  },
  {
    id: 'gps_neo6m',
    name: 'GPS NEO-6M',
    category: 'sensor',
    required_pins: [
      { name: 'tx', display_name: 'TX', required: true, description: '模块TX→ESP RX' },
      { name: 'rx', display_name: 'RX', required: true, description: '模块RX→ESP TX' },
    ],
    optional_pins: [],
    libraries: [{ id: 'tinygps', name: 'TinyGPS++' }],
  },
  {
    id: 'max30102',
    name: 'MAX30102 心率血氧',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [{ id: 'max30102_esp', name: 'MAX30102 Driver' }],
  },
  {
    id: 'sound_sensor',
    name: '声音传感器模块',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'hall_a3144',
    name: '霍尔传感器 A3144',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '数字输出引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO 输入/中断）' }],
  },
  {
    id: 'ldr',
    name: '光敏电阻 (LDR)',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '分压输出(ADC)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'rain_sensor',
    name: '雨滴/液位传感器',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'vl53l0x',
    name: 'VL53L0X 激光测距',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [
      { name: 'xshut', display_name: 'XSHUT', required: false, description: '关闭引脚' },
    ],
    libraries: [{ id: 'vl53l0x_esp', name: 'VL53L0X Driver' }],
  },
  {
    id: 'potentiometer',
    name: '电位器（模拟输入）',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '中间引脚(ADC)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'rotary_encoder',
    name: '旋转编码器 (KY-040)',
    category: 'other',
    required_pins: [
      { name: 'clk', display_name: 'CLK', required: true, description: '时钟/脉冲A' },
      { name: 'dt', display_name: 'DT', required: true, description: '数据/脉冲B' },
      { name: 'sw', display_name: 'SW', required: true, description: '按键' },
    ],
    optional_pins: [],
    libraries: [{ id: 'rotary_encoder', name: 'Rotary Encoder Library' }],
  },
  {
    id: 'ir_receiver',
    name: '红外接收 VS1838B',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '信号输出引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'ir_remote', name: 'IRremote Library' }],
  },

  // ==================== 显示设备 ====================
  {
    id: 'oled_ssd1306',
    name: 'OLED SSD1306 (0.96寸)',
    category: 'display',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [
      { name: 'res', display_name: 'RES', required: false, description: '复位引脚' },
    ],
    libraries: [
      { id: 'u8g2', name: 'U8g2' },
      { id: 'ssd1306_esp', name: 'SSD1306 Driver' },
    ],
  },
  {
    id: 'oled_sh1106',
    name: 'OLED SH1106 (1.3寸)',
    category: 'display',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [{ id: 'u8g2', name: 'U8g2' }],
  },
  {
    id: 'lcd1602_i2c',
    name: 'LCD1602 (I2C)',
    category: 'display',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'lcd_i2c', name: 'LiquidCrystal I2C' },
    ],
  },
  {
    id: 'lcd2004_i2c',
    name: 'LCD2004 (I2C)',
    category: 'display',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'lcd_i2c', name: 'LiquidCrystal I2C' },
    ],
  },
  {
    id: 'ili9341',
    name: 'ILI9341 TFT LCD (2.8寸)',
    category: 'display',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'dc', display_name: 'DC', required: true, description: '数据/命令' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [
      { name: 'miso', display_name: 'MISO', required: false, description: 'SPI 主入从出' },
      { name: 'bl', display_name: 'BL', required: false, description: '背光控制' },
    ],
    libraries: [
      { id: 'tft_espi', name: 'TFT_eSPI' },
      { id: 'lvgl', name: 'LVGL Graphics' },
    ],
  },
  {
    id: 'st7789',
    name: 'ST7789 TFT LCD',
    category: 'display',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'dc', display_name: 'DC', required: true, description: '数据/命令' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [
      { name: 'bl', display_name: 'BL', required: false, description: '背光控制' },
    ],
    libraries: [
      { id: 'tft_espi', name: 'TFT_eSPI' },
      { id: 'lvgl', name: 'LVGL Graphics' },
    ],
  },
  {
    id: 'max7219',
    name: 'MAX7219 LED 矩阵 (8x8)',
    category: 'display',
    required_pins: [
      { name: 'din', display_name: 'DIN', required: true, description: 'SPI 数据输入' },
      { name: 'cs', display_name: 'CS', required: true, description: '片选' },
      { name: 'clk', display_name: 'CLK', required: true, description: '时钟' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'max7219', name: 'MAX7219 Library' },
      { id: 'md_parola', name: 'MD_Parola' },
    ],
  },
  {
    id: 'tm1637',
    name: 'TM1637 4位数码管',
    category: 'display',
    required_pins: [
      { name: 'clk', display_name: 'CLK', required: true, description: '时钟引脚' },
      { name: 'dio', display_name: 'DIO', required: true, description: '数据引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'tm1637', name: 'TM1637 Library' }],
  },
  {
    id: 'ws2812b',
    name: 'WS2812B RGB灯带 (NeoPixel)',
    category: 'display',
    required_pins: [
      { name: 'din', display_name: 'DIN', required: true, description: '数据输入引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'fastled', name: 'FastLED' },
      { id: 'neo_pixel', name: 'Adafruit NeoPixel' },
      { id: 'esp_rmt', name: 'ESP32 RMT' },
    ],
  },
  {
    id: 'apa102',
    name: 'APA102 RGB灯带 (SPI)',
    category: 'display',
    required_pins: [
      { name: 'data', display_name: 'DATA', required: true, description: '数据引脚' },
      { name: 'clk', display_name: 'CLK', required: true, description: '时钟引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'fastled', name: 'FastLED' }],
  },

  // ==================== 通信模块 ====================
  {
    id: 'hc05_bluetooth',
    name: 'HC-05/HC-06 蓝牙串口',
    category: 'communication',
    required_pins: [
      { name: 'tx', display_name: 'TX', required: true, description: '模块TX→ESP RX' },
      { name: 'rx', display_name: 'RX', required: true, description: '模块RX→ESP TX' },
    ],
    optional_pins: [
      { name: 'en', display_name: 'EN/KEY', required: false, description: 'AT模式使能' },
    ],
    libraries: [{ id: 'none', name: '无（UART）' }],
  },
  {
    id: 'nrf24l01',
    name: 'NRF24L01 2.4G无线',
    category: 'communication',
    required_pins: [
      { name: 'ce', display_name: 'CE', required: true, description: '芯片使能' },
      { name: 'csn', display_name: 'CSN', required: true, description: 'SPI 片选' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'miso', display_name: 'MISO', required: true, description: 'SPI 主入从出' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [
      { name: 'irq', display_name: 'IRQ', required: false, description: '中断引脚' },
    ],
    libraries: [
      { id: 'rf24', name: 'RF24 Library' },
    ],
  },
  {
    id: 'lora_sx1278',
    name: 'LoRa SX1278 (433MHz)',
    category: 'communication',
    required_pins: [
      { name: 'cs', display_name: 'CS/NSS', required: true, description: 'SPI 片选' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'dio0', display_name: 'DIO0', required: true, description: '中断0' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'miso', display_name: 'MISO', required: true, description: 'SPI 主入从出' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'radiolib', name: 'RadioLib' },
      { id: 'lora_esp', name: 'LoRa Component' },
    ],
  },
  {
    id: 'rs485_max485',
    name: 'RS485 (MAX485)',
    category: 'communication',
    required_pins: [
      { name: 'tx', display_name: 'TX/DI', required: true, description: '发送→DI' },
      { name: 'rx', display_name: 'RX/RO', required: true, description: '接收←RO' },
      { name: 'de_re', display_name: 'DE/RE', required: true, description: '发送/接收使能' },
    ],
    optional_pins: [],
    libraries: [{ id: 'modbus', name: 'Modbus Library' }],
  },
  {
    id: 'can_mcp2515',
    name: 'CAN Bus MCP2515',
    category: 'communication',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'miso', display_name: 'MISO', required: true, description: 'SPI 主入从出' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
      { name: 'int', display_name: 'INT', required: true, description: '中断引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'mcp_can', name: 'MCP_CAN Library' },
    ],
  },

  // ==================== 存储模块 ====================
  {
    id: 'sd_card_spi',
    name: 'Micro SD卡 (SPI)',
    category: 'storage',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'miso', display_name: 'MISO', required: true, description: 'SPI 主入从出' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'sd_mmc', name: 'SD/MMC Driver' },
    ],
  },
  {
    id: 'eeprom_at24cxx',
    name: 'EEPROM AT24CXX (I2C)',
    category: 'storage',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（I2C 读写）' }],
  },

  // ==================== 其他 ====================
  {
    id: 'keypad_4x4',
    name: '矩阵键盘 4x4',
    category: 'other',
    required_pins: [
      { name: 'r1', display_name: 'R1', required: true, description: '行1' },
      { name: 'r2', display_name: 'R2', required: true, description: '行2' },
      { name: 'r3', display_name: 'R3', required: true, description: '行3' },
      { name: 'r4', display_name: 'R4', required: true, description: '行4' },
      { name: 'c1', display_name: 'C1', required: true, description: '列1' },
      { name: 'c2', display_name: 'C2', required: true, description: '列2' },
      { name: 'c3', display_name: 'C3', required: true, description: '列3' },
      { name: 'c4', display_name: 'C4', required: true, description: '列4' },
    ],
    optional_pins: [],
    libraries: [{ id: 'keypad', name: 'Keypad Library' }],
  },
  {
    id: 'ir_transmitter',
    name: '红外发射管',
    category: 'other',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '发射引脚(PWM)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'ir_remote', name: 'IRremote Library' }],
  },
  {
    id: 'vibration_motor',
    name: '振动马达模块',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '控制引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO/PWM）' }],
  },
  {
    id: 'servo_mg996r',
    name: '大扭力舵机 MG996R',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: 'PWM 控制引脚' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'esp32_pwm', name: 'ESP32 MCPWM' },
      { id: 'ledc', name: 'ESP32 LEDC' },
    ],
  },
  {
    id: 'water_pump',
    name: '微型水泵/电机驱动',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '控制引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO+MOS驱动）' }],
  },

  // ==================== 摄像头 ====================
  {
    id: 'ov2640',
    name: 'OV2640 摄像头 (ESP32-CAM)',
    category: 'camera',
    required_pins: [
      { name: 'xclk', display_name: 'XCLK', required: true, description: '外部时钟' },
      { name: 'pclk', display_name: 'PCLK', required: true, description: '像素时钟' },
      { name: 'vsync', display_name: 'VSYNC', required: true, description: '场同步' },
      { name: 'href', display_name: 'HREF', required: true, description: '行同步' },
      { name: 'd7', display_name: 'D7/Y9', required: true, description: '数据7' },
      { name: 'd6', display_name: 'D6/Y8', required: true, description: '数据6' },
      { name: 'd5', display_name: 'D5/Y7', required: true, description: '数据5' },
      { name: 'd4', display_name: 'D4/Y6', required: true, description: '数据4' },
      { name: 'd3', display_name: 'D3/Y5', required: true, description: '数据3' },
      { name: 'd2', display_name: 'D2/Y4', required: true, description: '数据2' },
      { name: 'sda', display_name: 'SDA/SIOD', required: true, description: 'SCCB 数据' },
      { name: 'scl', display_name: 'SCL/SIOC', required: true, description: 'SCCB 时钟' },
    ],
    optional_pins: [
      { name: 'pwdn', display_name: 'PWDN', required: false, description: '掉电控制' },
      { name: 'reset', display_name: 'RESET', required: false, description: '摄像头复位' },
      { name: 'd1', display_name: 'D1/Y3', required: false, description: '数据1(JPEG模式选配)' },
      { name: 'd0', display_name: 'D0/Y2', required: false, description: '数据0(JPEG模式选配)' },
    ],
    libraries: [
      { id: 'esp32_camera', name: 'ESP32 Camera Driver' },
    ],
  },
  {
    id: 'ov7670',
    name: 'OV7670 摄像头 (FIFO)',
    category: 'camera',
    required_pins: [
      { name: 'xclk', display_name: 'XCLK', required: true, description: '外部时钟' },
      { name: 'vsync', display_name: 'VSYNC', required: true, description: '场同步' },
      { name: 'sda', display_name: 'SDA/SIOD', required: true, description: 'SCCB 数据' },
      { name: 'scl', display_name: 'SCL/SIOC', required: true, description: 'SCCB 时钟' },
      { name: 'd7', display_name: 'D7', required: true, description: '数据7' },
      { name: 'd6', display_name: 'D6', required: true, description: '数据6' },
      { name: 'd5', display_name: 'D5', required: true, description: '数据5' },
      { name: 'd4', display_name: 'D4', required: true, description: '数据4' },
      { name: 'd3', display_name: 'D3', required: true, description: '数据3' },
      { name: 'd2', display_name: 'D2', required: true, description: '数据2' },
      { name: 'd1', display_name: 'D1', required: true, description: '数据1' },
      { name: 'd0', display_name: 'D0', required: true, description: '数据0' },
    ],
    optional_pins: [
      { name: 'pclk', display_name: 'PCLK', required: false, description: '像素时钟' },
      { name: 'href', display_name: 'HREF', required: false, description: '行同步' },
    ],
    libraries: [
      { id: 'ov7670', name: 'OV7670 Library' },
    ],
  },

  // ==================== LCD/显示扩展 ====================
  {
    id: 'st7735',
    name: 'ST7735 TFT LCD (1.8寸)',
    category: 'display',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'dc', display_name: 'DC/A0', required: true, description: '数据/命令' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [
      { name: 'bl', display_name: 'BL', required: false, description: '背光' },
    ],
    libraries: [
      { id: 'tft_espi', name: 'TFT_eSPI' },
      { id: 'adafruit_st7735', name: 'Adafruit ST7735' },
    ],
  },
  {
    id: 'lcd1602_parallel',
    name: 'LCD1602 (并行)',
    category: 'display',
    required_pins: [
      { name: 'rs', display_name: 'RS', required: true, description: '寄存器选择' },
      { name: 'en', display_name: 'EN', required: true, description: '使能' },
      { name: 'd4', display_name: 'D4', required: true, description: '数据4' },
      { name: 'd5', display_name: 'D5', required: true, description: '数据5' },
      { name: 'd6', display_name: 'D6', required: true, description: '数据6' },
      { name: 'd7', display_name: 'D7', required: true, description: '数据7' },
    ],
    optional_pins: [
      { name: 'rw', display_name: 'RW', required: false, description: '读写(通常接地)' },
    ],
    libraries: [
      { id: 'lcd', name: 'LiquidCrystal' },
    ],
  },
  {
    id: 'epaper_29',
    name: '电子墨水屏 2.9寸 (Waveshare)',
    category: 'display',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'dc', display_name: 'DC', required: true, description: '数据/命令' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'busy', display_name: 'BUSY', required: true, description: '忙信号' },
      { name: 'mosi', display_name: 'MOSI/DIN', required: true, description: 'SPI 数据' },
      { name: 'sck', display_name: 'SCK/CLK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'epd_driver', name: 'EPD Driver' },
      { id: 'gdewy029', name: 'GDEW029 Driver' },
    ],
  },
  {
    id: 'epaper_42',
    name: '电子墨水屏 4.2寸 (Waveshare)',
    category: 'display',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'dc', display_name: 'DC', required: true, description: '数据/命令' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'busy', display_name: 'BUSY', required: true, description: '忙信号' },
      { name: 'mosi', display_name: 'MOSI/DIN', required: true, description: 'SPI 数据' },
      { name: 'sck', display_name: 'SCK/CLK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'epd_driver', name: 'EPD Driver' },
    ],
  },

  // ==================== 传感器扩展 ====================
  {
    id: 'sht30',
    name: 'SHT30/SHT31 高精度温湿度',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'sht3x', name: 'SHT3x Driver' },
      { id: 'adafruit_sht31', name: 'Adafruit SHT31' },
    ],
  },
  {
    id: 'mlx90614',
    name: 'MLX90614 红外测温',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'mlx90614', name: 'MLX90614 Library' },
    ],
  },
  {
    id: 'tcs34725',
    name: 'TCS34725 颜色识别',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [
      { name: 'led', display_name: 'LED', required: false, description: '补光灯控制' },
    ],
    libraries: [
      { id: 'tcs34725', name: 'TCS34725 Library' },
    ],
  },
  {
    id: 'ccs811',
    name: 'CCS811 CO2/VOC传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [
      { name: 'wake', display_name: 'WAKE', required: false, description: '唤醒引脚' },
    ],
    libraries: [
      { id: 'ccs811', name: 'CCS811 Driver' },
    ],
  },
  {
    id: 'sgp30',
    name: 'SGP30 空气质量传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'sgp30', name: 'SGP30 Driver' },
    ],
  },
  {
    id: 'acs712',
    name: 'ACS712 电流传感器',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'flame_sensor',
    name: '火焰传感器模块',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'pulse_sensor',
    name: '脉搏/心率传感器 (PulseSensor)',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'pulse_sensor', name: 'PulseSensor Library' },
    ],
  },
  {
    id: 'water_flow',
    name: '水流量传感器 YF-S201',
    category: 'sensor',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '脉冲输出引脚(中断)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（脉冲计数）' }],
  },
  {
    id: 'mq7',
    name: 'MQ-7 一氧化碳传感器',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'mq4',
    name: 'MQ-4 甲烷/天然气传感器',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [
      { name: 'do', display_name: 'DO', required: false, description: '数字输出(阈值)' },
    ],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'voltage_divider',
    name: '电压检测模块 (分压)',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '分压输出(ADC)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'joystick',
    name: '摇杆模块 (KY-023)',
    category: 'sensor',
    required_pins: [
      { name: 'vrx', display_name: 'VRX', required: true, description: 'X轴(ADC)' },
      { name: 'vry', display_name: 'VRY', required: true, description: 'Y轴(ADC)' },
      { name: 'sw', display_name: 'SW', required: true, description: '按键' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC+GPIO）' }],
  },

  // ==================== RTC 时钟 ====================
  {
    id: 'ds3231',
    name: 'DS3231 高精度RTC时钟',
    category: 'storage',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [
      { id: 'ds3231', name: 'DS3231 Library' },
      { id: 'rtclib', name: 'RTClib' },
    ],
  },

  // ==================== 执行器扩展 ====================
  {
    id: 'tb6612',
    name: 'TB6612 双路电机驱动',
    category: 'actuator',
    required_pins: [
      { name: 'pwma', display_name: 'PWMA', required: true, description: 'A路PWM速度控制' },
      { name: 'ain1', display_name: 'AIN1', required: true, description: 'A路方向1' },
      { name: 'ain2', display_name: 'AIN2', required: true, description: 'A路方向2' },
    ],
    optional_pins: [
      { name: 'pwmb', display_name: 'PWMB', required: false, description: 'B路PWM速度控制' },
      { name: 'bin1', display_name: 'BIN1', required: false, description: 'B路方向1' },
      { name: 'bin2', display_name: 'BIN2', required: false, description: 'B路方向2' },
      { name: 'stby', display_name: 'STBY', required: false, description: '待机控制' },
    ],
    libraries: [{ id: 'none', name: '无（PWM+GPIO）' }],
  },
  {
    id: 'solenoid_lock',
    name: '电磁锁',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: '控制引脚(MOS驱动)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（GPIO输出）' }],
  },
  {
    id: 'fan_motor',
    name: '散热风扇/电机',
    category: 'actuator',
    required_pins: [
      { name: 'gpio', display_name: 'GPIO', required: true, description: 'PWM调速引脚(MOS驱动)' },
    ],
    optional_pins: [
      { name: 'tach', display_name: 'TACH', required: false, description: '转速反馈(中断)' },
    ],
    libraries: [{ id: 'none', name: '无（PWM输出）' }],
  },

  // ==================== 输入扩展 ====================
  {
    id: 'keypad_3x4',
    name: '矩阵键盘 3x4',
    category: 'other',
    required_pins: [
      { name: 'r1', display_name: 'R1', required: true, description: '行1' },
      { name: 'r2', display_name: 'R2', required: true, description: '行2' },
      { name: 'r3', display_name: 'R3', required: true, description: '行3' },
      { name: 'r4', display_name: 'R4', required: true, description: '行4' },
      { name: 'c1', display_name: 'C1', required: true, description: '列1' },
      { name: 'c2', display_name: 'C2', required: true, description: '列2' },
      { name: 'c3', display_name: 'C3', required: true, description: '列3' },
    ],
    optional_pins: [],
    libraries: [{ id: 'keypad', name: 'Keypad Library' }],
  },

  // ==================== 音频 ====================
  {
    id: 'dfplayer_mini',
    name: 'DFPlayer Mini MP3播放',
    category: 'actuator',
    required_pins: [
      { name: 'tx', display_name: 'TX', required: true, description: '模块TX→ESP RX' },
      { name: 'rx', display_name: 'RX', required: true, description: '模块RX→ESP TX' },
    ],
    optional_pins: [],
    libraries: [{ id: 'dfplayer', name: 'DFRobotDFPlayerMini' }],
  },
  {
    id: 'max98357a',
    name: 'MAX98357A I2S功放',
    category: 'actuator',
    required_pins: [
      { name: 'bclk', display_name: 'BCLK', required: true, description: 'I2S 位时钟' },
      { name: 'lrclk', display_name: 'LRCLK', required: true, description: 'I2S 左右声道' },
      { name: 'dout', display_name: 'DOUT', required: true, description: 'I2S 数据输出' },
    ],
    optional_pins: [],
    libraries: [{ id: 'i2s_driver', name: 'ESP32 I2S Driver' }],
  },
  {
    id: 'ina219',
    name: 'INA219 电压电流传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [{ id: 'ina219', name: 'INA219 Library' }],
  },
  {
    id: 'ads1115',
    name: 'ADS1115 16位ADC模块',
    category: 'sensor',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [
      { name: 'alert', display_name: 'ALERT/RDY', required: false, description: '转换完成中断' },
    ],
    libraries: [{ id: 'ads1115', name: 'ADS1115 Library' }],
  },
  {
    id: 'pcf8574',
    name: 'PCF8574 I2C IO扩展',
    category: 'communication',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [
      { name: 'int', display_name: 'INT', required: false, description: '中断输出' },
    ],
    libraries: [{ id: 'pcf8574', name: 'PCF8574 Library' }],
  },
  {
    id: 'esp_now',
    name: 'ESP-NOW 无线通信 (板载)',
    category: 'communication',
    required_pins: [],
    optional_pins: [],
    libraries: [{ id: 'esp_now', name: 'ESP-NOW' }],
  },
  {
    id: 'pwm_controller',
    name: 'PCA9685 16路PWM',
    category: 'actuator',
    required_pins: [
      { name: 'sda', display_name: 'SDA', required: true, description: 'I2C 数据线' },
      { name: 'scl', display_name: 'SCL', required: true, description: 'I2C 时钟线' },
    ],
    optional_pins: [],
    libraries: [{ id: 'pca9685', name: 'PCA9685 Library' }],
  },
  {
    id: 'thermal_printer',
    name: '热敏打印机 (微型)',
    category: 'actuator',
    required_pins: [
      { name: 'tx', display_name: 'TX', required: true, description: 'ESP TX→打印机 RX' },
      { name: 'rx', display_name: 'RX', required: true, description: 'ESP RX→打印机 TX' },
    ],
    optional_pins: [],
    libraries: [{ id: 'thermal', name: 'Thermal Printer Library' }],
  },
  {
    id: 'weight_hx711',
    name: 'HX711 称重传感器',
    category: 'sensor',
    required_pins: [
      { name: 'sck', display_name: 'SCK', required: true, description: '时钟引脚' },
      { name: 'dt', display_name: 'DT', required: true, description: '数据引脚' },
    ],
    optional_pins: [],
    libraries: [{ id: 'hx711', name: 'HX711 Library' }],
  },
  {
    id: 'ph_sensor',
    name: 'pH值传感器模块',
    category: 'sensor',
    required_pins: [
      { name: 'ao', display_name: 'AO', required: true, description: '模拟输出(ADC)' },
    ],
    optional_pins: [],
    libraries: [{ id: 'none', name: '无（ADC 读取）' }],
  },
  {
    id: 'co2_mh_z19',
    name: 'MH-Z19 CO2传感器',
    category: 'sensor',
    required_pins: [
      { name: 'tx', display_name: 'TX', required: true, description: '模块TX→ESP RX' },
      { name: 'rx', display_name: 'RX', required: true, description: '模块RX→ESP TX' },
    ],
    optional_pins: [],
    libraries: [{ id: 'mh_z19', name: 'MH-Z19 Library' }],
  },
  {
    id: 'tft_ili9488',
    name: 'ILI9488 TFT LCD (3.5寸)',
    category: 'display',
    required_pins: [
      { name: 'cs', display_name: 'CS', required: true, description: 'SPI 片选' },
      { name: 'dc', display_name: 'DC', required: true, description: '数据/命令' },
      { name: 'rst', display_name: 'RST', required: true, description: '复位' },
      { name: 'mosi', display_name: 'MOSI', required: true, description: 'SPI 主出从入' },
      { name: 'sck', display_name: 'SCK', required: true, description: 'SPI 时钟' },
    ],
    optional_pins: [
      { name: 'miso', display_name: 'MISO', required: false, description: 'SPI 主入从出' },
      { name: 'bl', display_name: 'BL', required: false, description: '背光控制' },
    ],
    libraries: [
      { id: 'tft_espi', name: 'TFT_eSPI' },
      { id: 'lvgl', name: 'LVGL Graphics' },
    ],
  },
];
