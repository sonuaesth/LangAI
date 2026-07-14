import type { Metadata } from "next";
import "./globals.css";
export const metadata: Metadata = { title: "LangAI", description: "Тренажёр переводов" };
export default function Layout({children}:{children:React.ReactNode}) { return <html lang="ru"><body>{children}</body></html>; }
