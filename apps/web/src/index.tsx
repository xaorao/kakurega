import { render } from "solid-js/web";
import { App } from "./App";
// the brand's typefaces, bundled with the page (SIL Open Font License 1.1): nothing is fetched from elsewhere
import "@fontsource/outfit/latin-400.css";
import "@fontsource/outfit/latin-500.css";
import "@fontsource/outfit/latin-600.css";
import "@fontsource/jetbrains-mono/latin-400.css";
import "./styles.css";

render(() => <App />, document.getElementById("root")!);
