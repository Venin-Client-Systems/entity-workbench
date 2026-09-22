import { useEffect, useRef } from "react";
import cytoscape from "cytoscape";
import * as maplibregl from "maplibre-gl";
import mapWorkerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
maplibregl.setWorkerUrl(mapWorkerUrl);
import * as echarts from "echarts/core";
import { BarChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { Workspace, Analysis } from "./types";
import "maplibre-gl/dist/maplibre-gl.css";
echarts.use([BarChart, GridComponent, TooltipComponent, CanvasRenderer]);
export function Graph({
  workspace,
  onSelect,
}: {
  workspace: Workspace;
  onSelect: (id: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const graph = cytoscape({
      container: ref.current,
      elements: [
        ...workspace.entities.map((e) => ({
          data: { id: e.id, label: e.name, kind: e.kind },
        })),
        ...workspace.assertions.map((a) => ({
          data: {
            id: a.id,
            source: a.subject_id,
            target: a.object_id,
            label: a.predicate,
          },
        })),
      ],
      style: [
        {
          selector: "node",
          style: {
            "background-color": "#176b63",
            label: "data(label)",
            color: "#17363a",
            "font-size": 13,
            "text-valign": "bottom",
            "text-margin-y": 12,
            width: 40,
            height: 40,
          },
        },
        {
          selector: 'node[kind="organisation"]',
          style: { shape: "round-rectangle", "background-color": "#c37f39" },
        },
        {
          selector: "edge",
          style: {
            width: 2,
            "line-color": "#92ada6",
            "curve-style": "bezier",
            "line-style": "dashed",
            label: "data(label)",
            "font-size": 11,
            color: "#667878",
            "text-rotation": "autorotate",
            "text-margin-y": -12,
          },
        },
      ],
      layout: { name: "circle", padding: 80 },
    });
    graph.on("tap", "node", (event) => onSelect(event.target.id()));
    return () => graph.destroy();
  }, [workspace.entities, workspace.assertions, onSelect]);
  return (
    <>
      <div
        className="graph"
        role="region"
        ref={ref}
        aria-label="Entity relationships"
      />
      <div className="actions" aria-label="Inspect graph entities">
        {workspace.entities.map((e) => (
          <button key={e.id} className="button" onClick={() => onSelect(e.id)}>
            {e.name} · {e.identifiers.map((i) => i.value).join(", ")}
          </button>
        ))}
      </div>
      <ul>
        {workspace.assertions.map((a) => (
          <li key={a.id}>
            {workspace.entities.find((e) => e.id === a.subject_id)?.name} →{" "}
            {a.predicate} →{" "}
            {workspace.entities.find((e) => e.id === a.object_id)?.name} ·{" "}
            {a.review}
          </li>
        ))}
      </ul>
    </>
  );
}
export function LocalMap({ workspace }: { workspace: Workspace }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const map = new maplibregl.Map({
      container: ref.current,
      center: [138.612, -34.908],
      zoom: 12,
      attributionControl: false,
      style: {
        version: 8,
        sources: {},
        layers: [
          {
            id: "background",
            type: "background",
            paint: { "background-color": "#e7eeea" },
          },
        ],
      },
      transformRequest: () => {
        throw new Error("External map requests are disabled");
      },
    });
    const markers: maplibregl.Marker[] = [];
    const points = [
      ...workspace.addresses.map((a) => ({
        lon: a.longitude,
        lat: a.latitude,
        label: a.label,
        color: "#176b63",
      })),
      ...workspace.locations
        .filter((l) => l.latitude !== null && l.longitude !== null)
        .map((l) => ({
          lon: l.longitude!,
          lat: l.latitude!,
          label: `${l.merchant} · ${l.branch} · unresolved`,
          color: "#bd7a35",
        })),
    ];
    points.forEach((p) => {
      markers.push(
        new maplibregl.Marker({ color: p.color })
          .setLngLat([p.lon, p.lat])
          .setPopup(new maplibregl.Popup().setText(p.label))
          .addTo(map),
      );
    });
    map.addControl(new maplibregl.NavigationControl(), "top-right");
    return () => {
      markers.forEach((m) => m.remove());
      map.remove();
    };
  }, [workspace.addresses, workspace.locations]);
  return (
    <div
      className="map"
      ref={ref}
      aria-label="Local coordinate map with no external basemap"
    />
  );
}
export function TotalsChart({
  analysis,
  onCurrency,
}: {
  analysis: Analysis;
  onCurrency: (currency: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      animation: false,
      grid: { top: 20, left: 50, right: 20, bottom: 35 },
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: analysis.totals.map((t) => t.currency) },
      yAxis: { type: "value" },
      series: [
        {
          name: "Credits",
          type: "bar",
          data: analysis.totals.map((t) => Number(t.credits)),
          itemStyle: { color: "#176b63", borderRadius: [4, 4, 0, 0] },
        },
        {
          name: "Debits",
          type: "bar",
          data: analysis.totals.map((t) => Number(t.debits)),
          itemStyle: { color: "#c18b48", borderRadius: [4, 4, 0, 0] },
        },
      ],
    });
    chart.on("click", (p) => onCurrency(p.name));
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(ref.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [analysis, onCurrency]);
  return (
    <>
      <div
        className="chart"
        role="region"
        ref={ref}
        aria-label="Reviewed credits and debits by currency"
      />
      <div className="table-scroll">
        <table>
          <caption>
            Exact reviewed totals; select a currency to inspect transactions
          </caption>
          <thead>
            <tr>
              <th>Currency</th>
              <th className="numeric">Credits</th>
              <th className="numeric">Debits</th>
              <th className="numeric">Net</th>
            </tr>
          </thead>
          <tbody>
            {analysis.totals.map((t) => (
              <tr key={t.currency}>
                <td>
                  <button
                    className="text-button"
                    onClick={() => onCurrency(t.currency)}
                  >
                    {t.currency}
                  </button>
                </td>
                <td className="numeric">{t.credits}</td>
                <td className="numeric">{t.debits}</td>
                <td className="numeric">{t.net}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
