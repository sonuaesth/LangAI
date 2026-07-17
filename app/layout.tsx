import type { Metadata } from "next";
import "./globals.css";
import "./exercise.css";
import "./sidebar.css";
import "./unified-theme.css";
import "./validation.css";
import "./punctuation.css";
import "./language.css";
import "./comments.css";
export const metadata: Metadata = { title: "LangAI", description: "Тренажёр переводов" };
export default function Layout({children}:{children:React.ReactNode}) { return <html lang="ru"><body>{children}</body></html>; }
