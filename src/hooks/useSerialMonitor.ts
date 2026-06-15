import { useState, useRef, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { showToast } from '../components/ui/Toast';
import { safeInvoke } from '../lib/invoke';

interface UseSerialMonitorOptions {
  selectedPort: string;
}

export function useSerialMonitor(options: UseSerialMonitorOptions) {
  const { t } = useTranslation();
  const { selectedPort } = options;

  const [serialOutput, setSerialOutput] = useState<string[]>([]);
  const [serialInput, setSerialInput] = useState('');
  const [serialConnected, setSerialConnected] = useState(false);
  const [serialBaudRate, setSerialBaudRate] = useState('115200');
  const serialBufferRef = useRef<string[]>([]);

  useEffect(() => {
    const interval = setInterval(() => {
      if (serialBufferRef.current.length > 0) {
        const batch = serialBufferRef.current;
        serialBufferRef.current = [];
        setSerialOutput((prev) => [...prev, ...batch].slice(-2000));
      }
    }, 100);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const unsubData = await listen<{ port: string; data: string }>('serial-data', (event) => {
          const lines = event.payload.data.split('\n');
          serialBufferRef.current.push(...lines);
        });
        const unsubDisconnect = await listen<{ port: string; error: string; reason: string }>('serial-disconnected', (event) => {
          setSerialConnected(false);
          setSerialOutput((prev) => [...prev, `[断开] ${event.payload.port}: ${event.payload.error || event.payload.reason || ''}`]);
        });
        unlisten = () => { unsubData(); unsubDisconnect(); };
      } catch { /* serial events may not be available */ }
    })();
    return () => { unlisten?.(); };
  }, []);

  const handleSerialConnect = useCallback(async () => {
    if (serialConnected) {
      try {
        await safeInvoke('close_serial_port');
      } catch { /* ignore */ }
      setSerialConnected(false);
      setSerialOutput((prev) => [...prev, '', '--- Disconnected ---', '']);
      return;
    }
    const port = selectedPort;
    if (!port) {
      showToast('warning', t('toast.selectSerialPort'));
      return;
    }
    try {
      await safeInvoke('open_serial_port', {
        app: null,
        port,
        baudrate: parseInt(serialBaudRate),
      });
      setSerialConnected(true);
      setSerialOutput((prev) => [...prev, '', `--- Connected to ${port} @ ${serialBaudRate} ---`, '']);
    } catch (err) {
      setSerialOutput((prev) => [...prev, '', `❌ Connection failed: ${err}`, '']);
    }
  }, [serialConnected, selectedPort, serialBaudRate, t]);

  const handleSerialSend = useCallback(async () => {
    if (!serialInput.trim() || !serialConnected) return;
    try {
      await safeInvoke('write_serial', { data: serialInput + '\n' });
      setSerialOutput((prev) => [...prev, `> ${serialInput}`]);
    } catch (err) {
      setSerialOutput((prev) => [...prev, '', `❌ Send failed: ${err}`, '']);
    }
    setSerialInput('');
  }, [serialInput, serialConnected]);

  return {
    serialOutput,
    setSerialOutput,
    serialInput,
    setSerialInput,
    serialConnected,
    setSerialConnected,
    serialBaudRate,
    setSerialBaudRate,
    serialBufferRef,
    handleSerialConnect,
    handleSerialSend,
  };
}
