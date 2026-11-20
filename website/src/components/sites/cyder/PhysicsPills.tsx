"use client";

import { useEffect, useRef, type ReactNode } from "react";
import Matter from "matter-js";

export type PhysicsItem = { key: string; label?: string; icon?: ReactNode; x: number; rot: number };

/* Pills and stickers that drop in when the section scrolls into view, then can be grabbed and thrown.
   Matter.js runs the bodies; the DOM elements just follow them. */
export function PhysicsPills({ items, label }: { items: PhysicsItem[]; label: string }) {
  const box = useRef<HTMLUListElement>(null);

  useEffect(() => {
    const root = box.current;
    if (!root) return;
    const { Engine, Bodies, Body, Composite, Constraint, Sleeping } = Matter;
    const els = [...root.querySelectorAll<HTMLElement>("[data-body]")];
    const engine = Engine.create({ enableSleeping: true, gravity: { x: 0, y: 1.1 } });
    const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;

    let width = root.clientWidth;
    let height = root.clientHeight;
    const T = 200; // wall thickness
    const H = 20000; // walls reach far above so thrown pieces come back down inside the box
    const floor = Bodies.rectangle(width / 2, height + T / 2, width * 3, T, { isStatic: true });
    const left = Bodies.rectangle(-T / 2, height - H / 2, T, H, { isStatic: true });
    const right = Bodies.rectangle(width + T / 2, height - H / 2, T, H, { isStatic: true });
    Composite.add(engine.world, [floor, left, right]);

    const bodies = els.map((el, i) => {
      const w = el.offsetWidth;
      const h = el.offsetHeight;
      const item = items[i];
      const body = Bodies.rectangle(item.x * width, -120 - i * 70, w, h, {
        angle: (item.rot * Math.PI) / 180,
        chamfer: { radius: Math.min(w, h) / 2 - 0.5 },
        restitution: 0.35,
        friction: 0.5,
        frictionAir: 0.012,
        density: 0.0015,
      });
      return { el, body, w, h };
    });

    const draw = () => {
      for (const { el, body, w, h } of bodies) {
        el.style.transform = `translate(${body.position.x - w / 2}px, ${body.position.y - h / 2}px) rotate(${body.angle}rad)`;
      }
    };

    // Drag: pin the grabbed point to the pointer with a stiff constraint; releasing keeps the body's velocity, so it flies.
    let drag: { constraint: Matter.Constraint; id: number } | null = null;
    const toLocal = (e: PointerEvent) => {
      const r = root.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const onDown = (e: PointerEvent) => {
      const idx = els.indexOf(e.currentTarget as HTMLElement);
      if (idx < 0 || drag) return;
      const { body } = bodies[idx];
      const p = toLocal(e);
      const local = Matter.Vector.rotate(Matter.Vector.sub(p, body.position), -body.angle);
      const constraint = Constraint.create({ pointA: p, bodyB: body, pointB: local, stiffness: 0.2, damping: 0.1, length: 0 });
      Composite.add(engine.world, constraint);
      Sleeping.set(body, false);
      drag = { constraint, id: e.pointerId };
      const el = e.currentTarget as HTMLElement;
      el.dataset.grabbed = "";
      try {
        el.setPointerCapture(e.pointerId); // keeps the drag alive when the pointer outruns the pill
      } catch {}
    };
    const onMove = (e: PointerEvent) => {
      if (drag && e.pointerId === drag.id) drag.constraint.pointA = toLocal(e);
    };
    const onUp = (e: PointerEvent) => {
      if (!drag || e.pointerId !== drag.id) return;
      Composite.remove(engine.world, drag.constraint);
      drag = null;
      delete (e.currentTarget as HTMLElement).dataset.grabbed;
    };
    els.forEach((el) => {
      el.addEventListener("pointerdown", onDown);
      el.addEventListener("pointermove", onMove);
      el.addEventListener("pointerup", onUp);
      el.addEventListener("pointercancel", onUp);
    });

    const onResize = () => {
      width = root.clientWidth;
      height = root.clientHeight;
      Body.setPosition(floor, { x: width / 2, y: height + T / 2 });
      Body.setPosition(left, { x: -T / 2, y: height - H / 2 });
      Body.setPosition(right, { x: width + T / 2, y: height - H / 2 });
      bodies.forEach(({ body }) => Sleeping.set(body, false));
    };
    addEventListener("resize", onResize);

    // Fixed 60 Hz steps, so the fall runs in real time even when frames are slow.
    const STEP = 1000 / 60;
    let raf = 0;
    let last = 0;
    let acc = 0;
    let started = false;
    let visible = false;
    const tick = (t: number) => {
      acc += last ? Math.min(t - last, 250) : STEP;
      last = t;
      while (acc >= STEP) {
        Engine.update(engine, STEP);
        acc -= STEP;
      }
      draw();
      raf = requestAnimationFrame(tick);
    };
    const start = () => {
      if (started) return;
      started = true;
      Composite.add(engine.world, bodies.map((b) => b.body));
      if (reduce) for (let i = 0; i < 600; i++) Engine.update(engine, 16.7); // settle instantly
      root.dataset.live = "";
      draw();
    };
    const io = new IntersectionObserver(
      ([e]) => {
        visible = e.isIntersecting;
        if (visible) start();
        cancelAnimationFrame(raf);
        last = 0;
        if (visible) raf = requestAnimationFrame(tick);
      },
      { threshold: 0.2 },
    );
    io.observe(root);

    return () => {
      io.disconnect();
      cancelAnimationFrame(raf);
      removeEventListener("resize", onResize);
      els.forEach((el) => {
        el.removeEventListener("pointerdown", onDown);
        el.removeEventListener("pointermove", onMove);
        el.removeEventListener("pointerup", onUp);
        el.removeEventListener("pointercancel", onUp);
      });
      Engine.clear(engine);
    };
  }, [items]);

  return (
    <ul ref={box} aria-label={label} className="group/p absolute inset-0 z-10">
      {items.map((p) => (
        <li
          key={p.key}
          data-body
          className="absolute top-0 left-0 cursor-grab touch-none opacity-0 select-none group-data-live/p:opacity-100 data-grabbed:cursor-grabbing"
        >
          {p.label ? (
            <span className="block rounded-full bg-white px-3 py-1.5 font-heading text-xs whitespace-nowrap text-black shadow-[0_8px_20px_rgba(0,0,0,0.45)] sm:px-6 sm:py-3 sm:text-base">
              {p.label}
            </span>
          ) : (
            <span aria-hidden="true" className="grid size-10 place-items-center rounded-2xl bg-white shadow-[0_8px_24px_rgba(0,0,0,0.35)] ring-4 ring-white sm:size-16">
              {p.icon}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}
