import { useEffect, useRef, useState } from 'react';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import { GridComponent, TooltipComponent, DataZoomComponent, MarkLineComponent, MarkPointComponent, AriaComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';

echarts.use([LineChart, GridComponent, TooltipComponent, DataZoomComponent, MarkLineComponent, MarkPointComponent, AriaComponent, CanvasRenderer]);

export default function Chart({ points, color = 'var(--metric-cpu)', max, unit = '%', height = 250, zoom = false, compact = false, label = 'Performance chart' }: {
  points: [number, number | null][]; color?: string; max?: number; unit?: string;
  height?: number; zoom?: boolean; compact?: boolean; label?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.EChartsType | null>(null);
  const [theme, setTheme] = useState(document.documentElement.dataset.theme);
  useEffect(() => {
    if (!ref.current) return;
    const c = echarts.init(ref.current);
    chart.current = c;
    const resize = new ResizeObserver(() => c.resize());
    resize.observe(ref.current);
    const observer = new MutationObserver(() => setTheme(document.documentElement.dataset.theme));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => { observer.disconnect(); resize.disconnect(); c.dispose(); chart.current = null; };
  }, []);
  useEffect(() => {
    const styles = getComputedStyle(document.documentElement);
    const token = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
    const line = color.startsWith('var(') ? token(color.slice(4, -1), '#7acdbb') : color;
    const muted = token('--muted', '#8e9fa7');
    const border = token('--line', '#29343a');
    chart.current?.setOption({
      animation: false,
      aria: { enabled: true, description: label },
      tooltip: {
        trigger: 'axis', confine: true,
        backgroundColor: token('--panel', '#151d21'), borderColor: border,
        textStyle: { color: token('--text', '#e2ebee'), fontFamily: 'IBM Plex Sans', fontSize: 12 },
        extraCssText: 'border-radius:8px;padding:10px 14px;box-shadow:0 8px 28px #0002',
        axisPointer: { lineStyle: { color: line, opacity: .45, type: 'dashed' } },
        valueFormatter: (v: unknown) => v == null ? 'No data' : Number(v).toFixed(1) + ' ' + unit,
      },
      grid: { top: compact ? 3 : 18, right: compact ? 0 : 16, left: compact ? 0 : 45, bottom: compact ? 0 : zoom ? 55 : 25 },
      xAxis: {
        type: 'time', show: !compact,
        min: points.length ? points[0][0] - (points.length < 2 ? 60000 : 0) : Date.now() - 60000,
        max: points.length ? points[points.length - 1][0] : Date.now(),
        axisLine: { show: false }, axisTick: { show: false },
        axisLabel: { color: muted, fontSize: 10, fontFamily: 'IBM Plex Mono', formatter: '{HH}:{mm}' },
        splitLine: { show: false },
      },
      yAxis: {
        type: 'value', show: !compact, min: 0, ...(max ? { max } : {}),
        axisLabel: { color: muted, fontSize: 10, fontFamily: 'IBM Plex Mono' },
        splitLine: { lineStyle: { color: border, type: 'dashed', opacity: .65 } },
      },
      dataZoom: zoom ? [{ type: 'inside' }, {
        type: 'slider', height: 18, bottom: 5, borderColor: border, fillerColor: line + '18',
        handleStyle: { color: line, borderColor: line }, textStyle: { color: muted },
        dataBackground: { lineStyle: { color: line, opacity: .4 }, areaStyle: { color: line, opacity: .08 } },
      }] : [],
      series: [{
        type: 'line', data: points, showSymbol: points.length === 1, symbolSize: 5,
        connectNulls: false, itemStyle: { color: line },
        lineStyle: { width: compact ? 1.8 : 2.2, color: line, cap: 'round', join: 'round' },
        areaStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
          { offset: 0, color: line + '26' }, { offset: 1, color: line + '00' },
        ]) },
      }],
    }, { replaceMerge: ['dataZoom'] });
  }, [points, color, max, unit, zoom, label, compact, theme]);
  return <div ref={ref} role="img" aria-label={label} style={{ height, width: '100%' }} />;
}
