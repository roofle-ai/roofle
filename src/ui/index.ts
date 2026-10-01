// Assembles the app shell from modular HTML partials. Runs as a side effect at
// import time so main.ts's top-level DOM queries find the injected nodes.
import "../tailwind.css";

import roofleIcon from "../assets/roofle-meeting.svg?raw";
import app from "./partials/app.html?raw";
import header from "./partials/header.html?raw";
import history from "./partials/history.html?raw";
import questions from "./partials/questions.html?raw";
import transcript from "./partials/transcript.html?raw";

/// Inject the header + three-pane workspace into `#app`.
export function mountApp(): void {
  const root = document.getElementById("app");
  if (!root) return;

  root.innerHTML = app
    .replace("<!--@header-->", header.replace("<!--@roofle-icon-->", roofleIcon))
    .replace("<!--@workspace-->", history + transcript + questions);
}

mountApp();
