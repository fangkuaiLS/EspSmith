/**
 * HardwareConfigTable - 项目硬件配置表
 *
 * 功能：
 * - 以表格形式显示当前项目所有已配置外设
 * - 添加/编辑外设（类型选择、引脚配置、库选择、备注）
 * - 相同类型硬件自动递增ID（如 led, led_2, led_3）
 * - AI 可通过 Tauri 命令读写此配置表
 */

import { useState, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Cpu, Plus, Trash2, Edit3, ChevronLeft, Search, Info, Wrench,
  // 分类图标
  Thermometer, Monitor, Zap, Wifi, HardDrive, Camera, Puzzle,
  // 具体硬件图标
  Lightbulb, Palette, Plug, Disc, Volume2, Gauge,
  Droplets, Wind, Eye, Radiation, Magnet, Sun,
  Fingerprint, Navigation, Heart, Ear, Signal,
  Radio, Satellite, Usb, Hash, Power, Fan, Lock, Droplet,
  Grid3x3,
} from 'lucide-react';
import { useHardwareStore, useProjectStore } from '../../stores';
import { PRESET_PERIPHERALS, PeripheralDefinition, PeripheralInstance, PeripheralUpdate } from '../../types';

function PeripheralForm({
  peripheral,
  initialValues,
  onSave,
  onCancel,
}: {
  peripheral: PeripheralDefinition;
  initialValues?: PeripheralInstance;
  onSave: (instance: PeripheralInstance) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const isEdit = !!initialValues;

  const [name, setName] = useState(initialValues?.name || '');
  const [pinValues, setPinValues] = useState<Record<string, string>>(
    initialValues
      ? Object.fromEntries(
          Object.entries(initialValues.pin_values).map(([k, v]) => [k, String(v)])
        )
      : {}
  );
  const hasInitialCustomLib = initialValues?.library_choice && 
    !peripheral.libraries.some(lib => lib.id === initialValues.library_choice);
  const [library, setLibrary] = useState(
    hasInitialCustomLib ? 'custom' : (initialValues?.library_choice || peripheral.libraries[0]?.id || 'none')
  );
  const [customLibrary, setCustomLibrary] = useState(hasInitialCustomLib ? initialValues?.library_choice || '' : '');
  const [notes, setNotes] = useState(initialValues?.notes || '');

  const allPins = [...peripheral.required_pins, ...peripheral.optional_pins];

  const handleSave = () => {
    const finalLibrary = library === 'custom' ? customLibrary.trim() || 'none' : library;
    const instance: PeripheralInstance = {
      id: initialValues?.id || `${peripheral.id}-${Date.now()}`,
      definition_id: peripheral.id,
      name: name || t(`hardware.peripherals.${peripheral.id}`, peripheral.name),
      pin_values: Object.fromEntries(
        Object.entries(pinValues).map(([k, v]) => [k, parseInt(v) || 0])
      ),
      library_choice: finalLibrary,
      notes,
    };
    onSave(instance);
  };

  return (
    <div className="animate-slide-up">
      <button
        onClick={onCancel}
        className="flex items-center gap-1 text-[12px] text-text-tertiary hover:text-text-primary mb-4 transition-colors"
      >
        <ChevronLeft size={14} />
        {t('hardware.backToList')}
      </button>

      <h3 className="text-[14px] font-semibold mb-4">
        {isEdit ? t('hardware.editPeripheral', { name: initialValues?.name || '' }) : t('hardware.addPeripheral')}
      </h3>

      <div className="space-y-4">
        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.peripheralType')}
          </label>
          <div className="px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary">
            {t(`hardware.peripherals.${peripheral.id}`, peripheral.name)}
          </div>
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.instanceName')}
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t(`hardware.peripherals.${peripheral.id}`, peripheral.name)}
            className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent"
          />
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-2 uppercase tracking-wider">
            {t('hardware.pinConfig')}
          </label>
          {allPins.map((pin) => (
            <div key={pin.name} className="flex items-center gap-2 mb-2">
              <span className="w-20 text-[12px] text-text-secondary shrink-0">
                {pin.display_name}
                {pin.required && <span className="text-error ml-0.5">*</span>}
              </span>
              <input
                type="number"
                min="0"
                max="48"
                value={pinValues[pin.name] || ''}
                onChange={(e) => setPinValues({ ...pinValues, [pin.name]: e.target.value })}
                placeholder={pin.required ? t('hardware.required') : t('hardware.optional')}
                className="flex-1 px-2.5 py-1.5 text-[12px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary font-mono placeholder:text-text-disabled focus:outline-none focus:border-accent"
              />
              <span className="text-[10px] text-text-disabled hidden sm:block w-16 shrink-0">
                {pin.description}
              </span>
            </div>
          ))}
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.driverLib')}
          </label>
          <select
            value={library}
            onChange={(e) => {
              setLibrary(e.target.value);
              if (e.target.value !== 'custom') {
                setCustomLibrary('');
              }
            }}
            className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary focus:outline-none focus:border-accent"
          >
            {peripheral.libraries.map((lib) => (
                <option key={lib.id} value={lib.id}>
                  {lib.id === 'none' ? t('hardware.libNone') : t(`hardware.libs.${lib.id}`, lib.name)}
                </option>
              ))}
            <option value="custom">-- {t('hardware.custom')} --</option>
          </select>
          {library === 'custom' && (
            <input
              type="text"
              value={customLibrary}
              onChange={(e) => setCustomLibrary(e.target.value)}
              placeholder={t('hardware.customLibraryPlaceholder')}
              className="w-full px-3 py-2 mt-2 text-[13px] bg-surface-overlay border border-accent/50 rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent"
            />
          )}
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.notes')}
          </label>
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder={t('hardware.notesPlaceholder')}
            rows={3}
            className="w-full px-3 py-2 text-[12px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent resize-none"
          />
        </div>
      </div>

      <div className="flex gap-2 justify-end mt-6">
        <button
          onClick={onCancel}
          className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-md text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors"
        >
          {t('hardware.cancel')}
        </button>
        <button
          onClick={handleSave}
          className="px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-md hover:bg-accent-hover transition-colors"
        >
          {isEdit ? t('hardware.save') : t('hardware.add')}
        </button>
      </div>
    </div>
  );
}

// 分类图标映射
const CATEGORY_ICONS: Record<string, React.ElementType> = {
  sensor: Thermometer,
  display: Monitor,
  actuator: Zap,
  communication: Wifi,
  storage: HardDrive,
  camera: Camera,
  other: Puzzle,
};

// 具体硬件图标映射（按 id）
const PERIPHERAL_ICONS: Record<string, React.ElementType> = {
  // 执行器
  led: Lightbulb,
  rgb_led: Palette,
  relay: Plug,
  servo: Gauge,
  stepper_28byj48: Disc,
  dc_motor_l298n: Power,
  buzzer: Volume2,
  vibration_motor: Signal,
  servo_mg996r: Gauge,
  water_pump: Droplet,
  tb6612: Power,
  solenoid_lock: Lock,
  fan_motor: Fan,
  // 传感器
  button: Power,
  dht11: Droplets,
  dht22: Droplets,
  ds18b20: Thermometer,
  bmp280: Wind,
  bme280: Wind,
  aht10: Droplets,
  mpu6050: Magnet,
  hc_sr04: Ear,
  bh1750: Sun,
  pir_hcsr501: Eye,
  mq2: Radiation,
  mq135: Wind,
  soil_moisture: Droplets,
  touch_ttp223: Fingerprint,
  rc522_rfid: Fingerprint,
  gps_neo6m: Navigation,
  max30102: Heart,
  sound_sensor: Ear,
  hall_a3144: Magnet,
  ldr: Sun,
  rain_sensor: Droplet,
  vl53l0x: Eye,
  potentiometer: Gauge,
  rotary_encoder: Disc,
  ir_receiver: Radio,
  sht30: Droplets,
  mlx90614: Thermometer,
  tcs34725: Palette,
  ccs811: Wind,
  sgp30: Wind,
  acs712: Gauge,
  flame_sensor: Radiation,
  pulse_sensor: Heart,
  water_flow: Droplet,
  mq7: Radiation,
  mq4: Radiation,
  voltage_divider: Gauge,
  joystick: Disc,
  // 显示
  oled_ssd1306: Monitor,
  oled_sh1106: Monitor,
  lcd1602_i2c: Hash,
  lcd2004_i2c: Hash,
  ili9341: Monitor,
  st7789: Monitor,
  max7219: Grid3x3,
  tm1637: Hash,
  ws2812b: Lightbulb,
  apa102: Lightbulb,
  st7735: Monitor,
  lcd1602_parallel: Hash,
  epaper_29: Monitor,
  epaper_42: Monitor,
  // 通信
  hc05_bluetooth: Wifi,
  nrf24l01: Radio,
  lora_sx1278: Satellite,
  rs485_max485: Usb,
  can_mcp2515: Usb,
  // 存储
  sd_card_spi: HardDrive,
  eeprom_at24cxx: HardDrive,
  ds3231: HardDrive,
  // 摄像头
  ov2640: Camera,
  ov7670: Camera,
  // 其他
  keypad_4x4: Grid3x3,
  keypad_3x4: Grid3x3,
  ir_transmitter: Radio,
  // 新增硬件
  dfplayer_mini: Volume2,
  max98357a: Volume2,
  ina219: Gauge,
  ads1115: Gauge,
  pcf8574: Plug,
  esp_now: Wifi,
  pwm_controller: Gauge,
  thermal_printer: Grid3x3,
  weight_hx711: Gauge,
  ph_sensor: Droplet,
  co2_mh_z19: Wind,
  tft_ili9488: Monitor,
};

function TypeSelector({
  onSelect,
  onManualAdd,
}: {
  onSelect: (peripheral: PeripheralDefinition) => void;
  onManualAdd: () => void;
}) {
  const { t } = useTranslation();
  const [search, setSearch] = useState('');
  const [activeCategory, setActiveCategory] = useState('all');

  const categories = ['all', 'sensor', 'display', 'actuator', 'communication', 'storage', 'camera', 'other'];

  const filtered = PRESET_PERIPHERALS.filter((p) => {
    const matchCategory = activeCategory === 'all' || p.category === activeCategory;
    const translatedName = t(`hardware.peripherals.${p.id}`, p.name);
    const matchSearch = !search ||
      translatedName.toLowerCase().includes(search.toLowerCase()) ||
      p.name.toLowerCase().includes(search.toLowerCase()) ||
      p.id.toLowerCase().includes(search.toLowerCase());
    return matchCategory && matchSearch;
  });

  return (
    <div className="animate-slide-up flex flex-col min-h-0 flex-1">
      <button
        onClick={() => onSelect(null as any)}
        className="flex items-center gap-1 text-[12px] text-text-tertiary hover:text-text-primary mb-3 transition-colors"
      >
        <ChevronLeft size={14} />
        {t('hardware.backToList')}
      </button>

      <h3 className="text-[14px] font-semibold mb-3">{t('hardware.selectType')}</h3>

      {/* 分类标签栏 */}
      <div className="flex flex-wrap gap-1.5 mb-3">
        {categories.map((cat) => {
          const CatIcon = CATEGORY_ICONS[cat] || Puzzle;
          const isActive = activeCategory === cat;
          return (
            <button
              key={cat}
              onClick={() => setActiveCategory(cat)}
              className={`flex items-center gap-1 px-2 py-1 text-[11px] font-medium rounded-md transition-colors ${
                isActive
                  ? 'bg-accent text-white'
                  : 'bg-surface-overlay text-text-tertiary border border-border-subtle hover:text-text-primary hover:border-border-default'
              }`}
            >
              <CatIcon size={12} />
              {t(`hardware.categories.${cat}`)}
            </button>
          );
        })}
      </div>

      <div className="relative mb-3">
        <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-disabled" />
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t('hardware.searchType')}
          className="w-full pl-8 pr-3 py-2 text-[12px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent"
        />
      </div>

      <div className="flex-1 min-h-0 space-y-1.5 overflow-y-auto">
        {filtered.map((p) => {
          const PeriphIcon = PERIPHERAL_ICONS[p.id] || CATEGORY_ICONS[p.category] || Info;
          return (
            <button
              key={p.id}
              onClick={() => onSelect(p)}
              className="w-full flex items-center gap-3 p-3 bg-surface-elevated rounded-lg border border-border-subtle hover:border-accent/40 hover:bg-surface-hover transition-all text-left"
            >
              <div className="w-8 h-8 rounded-lg bg-surface-overlay border border-border-subtle flex items-center justify-center shrink-0">
                <PeriphIcon size={15} className="text-text-secondary" />
              </div>
              <div className="min-w-0">
                <div className="text-[13px] font-medium text-text-primary">{t(`hardware.peripherals.${p.id}`, p.name)}</div>
                <div className="text-[11px] text-text-tertiary">
                  {p.required_pins.map((pin) => pin.display_name).join(', ')}
                </div>
              </div>
            </button>
          );
        })}
        {filtered.length === 0 && (
          <p className="text-center text-[12px] text-text-disabled py-4">{t('hardware.noTypeFound')}</p>
        )}
      </div>

      <div className="shrink-0 pt-3 border-t border-border-subtle">
        <button
          onClick={onManualAdd}
          className="w-full flex items-center justify-center gap-2 p-3 bg-accent/5 border border-accent/30 rounded-lg hover:bg-accent/10 hover:border-accent/50 transition-all text-left"
        >
          <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
            <Wrench size={15} className="text-accent" />
          </div>
          <div>
            <div className="text-[13px] font-medium text-accent">{t('hardware.manualAdd')}</div>
            <div className="text-[11px] text-text-tertiary">{t('hardware.manualAddHint')}</div>
          </div>
        </button>
      </div>
    </div>
  );
}

function ManualAddForm({
  initialValues,
  onSave,
  onCancel,
}: {
  initialValues?: PeripheralInstance;
  onSave: (instance: PeripheralInstance) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const isEdit = !!initialValues;

  const [name, setName] = useState(initialValues?.name || '');
  const [definitionId, setDefinitionId] = useState(initialValues?.definition_id || '');
  const [pinsInput, setPinsInput] = useState(
    initialValues
      ? Object.entries(initialValues.pin_values).map(([k, v]) => `${k}=${v}`).join(', ')
      : ''
  );
  const [library, setLibrary] = useState(initialValues?.library_choice || 'none');
  const [notes, setNotes] = useState(initialValues?.notes || '');

  const handleSave = () => {
    const pinMap: Record<string, number> = {};
    const pinPairs = pinsInput.split(',').filter(Boolean);
    for (const pair of pinPairs) {
      const [key, val] = pair.split('=').map((s) => s.trim());
      if (key && val && !isNaN(Number(val))) {
        pinMap[key] = parseInt(val);
      }
    }

    const instance: PeripheralInstance = {
      id: initialValues?.id || `${definitionId || 'custom'}-${Date.now()}`,
      definition_id: definitionId || 'custom',
      name: name || t('hardware.manualPeripheral'),
      pin_values: pinMap,
      library_choice: library,
      notes,
    };
    onSave(instance);
  };

  return (
    <div className="animate-slide-up">
      <button
        onClick={onCancel}
        className="flex items-center gap-1 text-[12px] text-text-tertiary hover:text-text-primary mb-4 transition-colors"
      >
        <ChevronLeft size={14} />
        {t('hardware.backToList')}
      </button>

      <h3 className="text-[14px] font-semibold mb-4">{isEdit ? t('hardware.editPeripheral', { name: initialValues?.name || '' }) : t('hardware.manualAdd')}</h3>

      <div className="space-y-4">
        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.instanceName')}
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('hardware.namePlaceholder')}
            className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent"
          />
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.typeId')}
          </label>
          <input
            type="text"
            value={definitionId}
            onChange={(e) => setDefinitionId(e.target.value)}
            placeholder={t('hardware.typeIdPlaceholder')}
            className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent"
          />
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.pinConfig')}
          </label>
          <input
            type="text"
            value={pinsInput}
            onChange={(e) => setPinsInput(e.target.value)}
            placeholder={t('hardware.pinsPlaceholder')}
            className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary font-mono placeholder:text-text-disabled focus:outline-none focus:border-accent"
          />
          <p className="text-[10px] text-text-disabled mt-1">{t('hardware.pinsHint')}</p>
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.driverLib')}
          </label>
          <input
            type="text"
            value={library}
            onChange={(e) => setLibrary(e.target.value)}
            placeholder="none"
            className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent"
          />
        </div>

        <div>
          <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">
            {t('hardware.notes')}
          </label>
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder={t('hardware.notesPlaceholder')}
            rows={3}
            className="w-full px-3 py-2 text-[12px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent resize-none"
          />
        </div>
      </div>

      <div className="flex gap-2 justify-end mt-6">
        <button
          onClick={onCancel}
          className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-md text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors"
        >
          {t('hardware.cancel')}
        </button>
        <button
          onClick={handleSave}
          className="px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-md hover:bg-accent-hover transition-colors"
        >
          {isEdit ? t('hardware.save') : t('hardware.add')}
        </button>
      </div>
    </div>
  );
}

export function HardwareStore() {
  const { t } = useTranslation();
  const [view, setView] = useState<'list' | 'selectType' | 'configure' | 'manual'>('list');
  const [selectedType, setSelectedType] = useState<PeripheralDefinition | null>(null);
  const [editingInstance, setEditingInstance] = useState<PeripheralInstance | null>(null);
  const {
    config,
    loadConfig,
    getNextId,
    addPeripheral,
    updatePeripheral,
    removePeripheral,
    checkConflicts,
  } = useHardwareStore();
  const { currentProject } = useProjectStore();

  useEffect(() => {
    if (currentProject?.path) {
      loadConfig(currentProject.path);
    }
  }, [currentProject?.path, loadConfig]);

  const peripherals = config ? Object.values(config.peripherals) : [];

  const handleSelectType = useCallback((peripheral: PeripheralDefinition) => {
    if (!peripheral) {
      setView('list');
      return;
    }
    setSelectedType(peripheral);
    setEditingInstance(null);
    setView('configure');
  }, []);

  const handleAddClick = useCallback(() => {
    setView('selectType');
    setSelectedType(null);
    setEditingInstance(null);
  }, []);

  const handleManualAdd = useCallback(() => {
    setView('manual');
    setSelectedType(null);
    setEditingInstance(null);
  }, []);

  const handleEditClick = useCallback((instance: PeripheralInstance) => {
    const def = PRESET_PERIPHERALS.find((p) => p.id === instance.definition_id);
    if (def) {
      setSelectedType(def);
      setEditingInstance(instance);
      setView('configure');
    } else {
      // Custom/manual peripheral: open manual edit form
      setSelectedType(null);
      setEditingInstance(instance);
      setView('manual');
    }
  }, []);

  const handleSavePeripheral = useCallback(
    async (instance: PeripheralInstance) => {
      if (!currentProject) return;

      if (editingInstance) {
        const update: PeripheralUpdate = {
          name: instance.name,
          pin_values: instance.pin_values,
          library_choice: instance.library_choice,
          notes: instance.notes,
        };
        try {
          await updatePeripheral(currentProject.path, editingInstance.id, update);
          setView('list');
          setSelectedType(null);
          setEditingInstance(null);
        } catch { /* error shown by store */ }
        return;
      }

      try {
        const nextId = await getNextId(currentProject.path, instance.definition_id);
        const newInstance = { ...instance, id: nextId };

        const conflicts = await checkConflicts(currentProject.path, newInstance);
        if (conflicts.length > 0) {
          const conflictMsg = conflicts
            .map((c) => `GPIO${c.pin}: ${c.peripheral_a} ↔ ${c.peripheral_b}`)
            .join('\n');
          alert(t('hardware.pinConflict', { conflicts: conflictMsg }));
          return;
        }

        await addPeripheral(currentProject.path, newInstance);
        setView('list');
        setSelectedType(null);
        setEditingInstance(null);
      } catch { /* error shown by store */ }
    },
    [currentProject, selectedType, editingInstance, getNextId, checkConflicts, addPeripheral, updatePeripheral]
  );

  const handleRemovePeripheral = useCallback(
    async (id: string) => {
      if (!currentProject) return;
      await removePeripheral(currentProject.path, id);
    },
    [currentProject, removePeripheral]
  );

  return (
    <div className="h-full flex flex-col bg-surface-base">
      <div className="px-4 py-3 border-b border-border-subtle flex items-center justify-between shrink-0">
        <h2 className="text-[13px] font-semibold">{t('hardware.title')}</h2>
        {view === 'list' && (
          <button
            onClick={handleAddClick}
            className="flex items-center gap-1 px-2.5 py-1.5 text-[11px] font-medium bg-accent text-white rounded-md hover:bg-accent-hover transition-colors"
          >
            <Plus size={13} />
            {t('hardware.addPeripheral')}
          </button>
        )}
      </div>

      <div className="flex-1 min-h-0 flex flex-col p-4">
        {!currentProject ? (
          <div className="flex flex-col items-center justify-center h-full text-center">
            <Cpu size={36} className="text-text-disabled mb-3" />
            <p className="text-[13px] text-text-tertiary mb-1">{t('hardware.noProject')}</p>
            <p className="text-[11px] text-text-disabled">{t('hardware.noProjectHint')}</p>
          </div>
        ) : view === 'selectType' ? (
          <TypeSelector onSelect={handleSelectType} onManualAdd={handleManualAdd} />
        ) : view === 'manual' ? (
          <ManualAddForm
            initialValues={editingInstance || undefined}
            onSave={handleSavePeripheral}
            onCancel={() => { setView('list'); setEditingInstance(null); }}
          />
        ) : view === 'configure' && selectedType ? (
          <PeripheralForm
            peripheral={selectedType}
            initialValues={editingInstance || undefined}
            onSave={handleSavePeripheral}
            onCancel={() => setView('list')}
          />
        ) : (
          <>
            {peripherals.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-center">
                <Cpu size={32} className="text-text-disabled mb-2" />
                <p className="text-[13px] text-text-tertiary mb-1">{t('hardware.emptyTable')}</p>
                <p className="text-[11px] text-text-disabled">{t('hardware.emptyTableHint')}</p>
              </div>
            ) : (
              <div className="space-y-2 overflow-y-auto flex-1 min-h-0">
                {peripherals.map((instance) => {
                  const def = PRESET_PERIPHERALS.find((p) => p.id === instance.definition_id);
                  const pinEntries = Object.entries(instance.pin_values);
                  return (
                    <div
                      key={instance.id}
                      className="bg-surface-elevated rounded-lg border border-border-subtle p-3 hover:border-border-default transition-colors"
                    >
                      <div className="flex items-start justify-between">
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2 mb-1.5">
                            <span className="text-[13px] font-semibold text-text-primary">
                              {instance.name}
                            </span>
                            <span className="px-1.5 py-0.5 text-[10px] font-mono bg-surface-overlay text-text-tertiary rounded border border-border-subtle">
                              {instance.id}
                            </span>
                            <span className="px-1.5 py-0.5 text-[10px] font-medium bg-accent/10 text-accent rounded flex items-center gap-1">
                              {(() => {
                                const PeriphIcon = PERIPHERAL_ICONS[instance.definition_id] || (def ? CATEGORY_ICONS[def.category] : null) || Cpu;
                                return <PeriphIcon size={12} className="text-accent shrink-0" />;
                              })()}
                              {def ? t(`hardware.peripherals.${def.id}`, def.name) : instance.definition_id}
                            </span>
                          </div>

                          <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-text-secondary mb-1">
                            {pinEntries.length > 0 && (
                              <span>
                                {t('hardware.pins')}:{' '}
                                {pinEntries.map(([k, v]) => `${k}=GPIO${v}`).join(', ')}
                              </span>
                            )}
                            <span>
                              {t('hardware.library')}: {instance.library_choice}
                            </span>
                          </div>

                          {instance.notes && (
                            <p className="text-[11px] text-text-tertiary mt-1.5 italic">
                              {t('hardware.notesLabel')}: {instance.notes}
                            </p>
                          )}
                        </div>

                        <div className="flex items-center gap-1 ml-2 shrink-0">
                          <button
                            onClick={() => handleEditClick(instance)}
                            className="p-1.5 rounded-md text-text-tertiary hover:text-accent hover:bg-surface-hover transition-colors"
                            title={t('hardware.edit')}
                          >
                            <Edit3 size={13} />
                          </button>
                          <button
                            onClick={() => handleRemovePeripheral(instance.id)}
                            className="p-1.5 rounded-md text-text-tertiary hover:text-error hover:bg-error-muted transition-colors"
                            title={t('hardware.delete')}
                          >
                            <Trash2 size={13} />
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export default HardwareStore;