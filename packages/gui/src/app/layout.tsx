import type { Metadata } from "next";
import localFont from "next/font/local";
import { JetBrains_Mono } from "next/font/google";
import "./globals.css";

const sora = localFont({
  src: [
    { path: "../../public/fonts/Sora-Variable.ttf", style: "normal", weight: "100 800" },
    { path: "../../public/fonts/Sora-VariableItalic.ttf", style: "italic", weight: "100 800" },
  ],
  variable: "--font-sans",
  display: "swap",
});

const fontMono = JetBrains_Mono({
  variable: "--font-mono",
  subsets: ["latin"],
});

import { AgentProvider } from "@/components/AgentProvider";

export const metadata: Metadata = {
  title: "Vivianne",
  description: "AI Agent",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      className={`${sora.variable} ${fontMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col font-sans bg-canvas text-ink">
        <AgentProvider>
          {children}
        </AgentProvider>
      </body>
    </html>
  );
}
