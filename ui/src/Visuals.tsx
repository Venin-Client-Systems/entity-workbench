import { useEffect, useRef } from "react";
import cytoscape from "cytoscape";
import * as maplibregl from "maplibre-gl";
import mapWorkerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
maplibregl.setWorkerUrl(mapWorkerUrl);
import * as echarts from "echarts/core";
import { BarChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { Workspace, LedgerSummary } from "./types";
import "maplibre-gl/dist/maplibre-gl.css";
echarts.use([BarChart, GridComponent, TooltipComponent, CanvasRenderer]);
export function Graph({
  workspace,
  onSelect,
}: {
  workspace: Pick<Workspace, "entities" | "assertions">;
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
            "background-color": "#394347",
            label: "data(label)",
            color: "#202729",
            "font-size": 13,
            "text-valign": "bottom",
            "text-margin-y": 12,
            width: 40,
            height: 40,
          },
        },
        {
          selector: 'node[kind="organisation"]',
          style: { shape: "rectangle", "background-color": "#a64814" },
        },
        {
          selector: "edge",
          style: {
            width: 2,
            "line-color": "#747f7c",
            "curve-style": "bezier",
            "line-style": "dashed",
            label: "data(label)",
            "font-size": 11,
            color: "#4f5c60",
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
      <div className="actions" role="group" aria-label="Inspect graph entities">
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
export function LocalMap({
  workspace,
}: {
  workspace: Pick<Workspace, "addresses" | "locations">;
}) {
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
            paint: { "background-color": "#e7eae8" },
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
        color: "#394347",
      })),
      ...workspace.locations
        .filter((l) => l.latitude !== null && l.longitude !== null)
        .map((l) => ({
          lon: l.longitude!,
          lat: l.latitude!,
          label: `${l.merchant} · ${l.branch} · unresolved`,
          color: "#a64814",
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
      role="region"
      ref={ref}
      aria-label="Local coordinate map with no external basemap"
    />
  );
}
export function TotalsChart({
  analysis,
  onCurrency,
}: {
  analysis: Pick<LedgerSummary, "totals">;
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
          itemStyle: { color: "#394347", borderRadius: 0 },
        },
        {
          name: "Debits",
          type: "bar",
          data: analysis.totals.map((t) => Number(t.debits)),
          itemStyle: { color: "#a64814", borderRadius: 0 },
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
