import type { ReactNode } from "react";
import type { Metadata } from "next";
import { Inter } from "next/font/google";
import "./globals.css";

const inter = Inter({
  variable: "--font-inter",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "KanVibe",
  description: "AI Agent Task Management Kanban",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html suppressHydrationWarning>
      <body className={`${inter.variable} font-sans bg-bg-page text-text-primary antialiased`}>
        {children}
      </body>
    </html>
  );
}
