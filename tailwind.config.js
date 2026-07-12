/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        brand: {
          DEFAULT: "#6D5DFC",
          light: "#8B7DFF",
          dark: "#4F46E5",
        },
        surface: {
          glass: "rgba(255, 255, 255, 0.15)",
          dark: "rgba(28, 28, 30, 0.72)",
        },
      },
      backdropBlur: {
        xs: "2px",
      },
      animation: {
        "pulse-glow": "pulseGlow 2s ease-in-out infinite",
        "waveform": "waveform 1.6s ease-in-out infinite",
        "float": "float 3s ease-in-out infinite",
        "fade-in": "fadeIn 0.3s ease-out",
        "scale-in": "scaleIn 0.35s cubic-bezier(0.34, 1.56, 0.64, 1)",
      },
      keyframes: {
        pulseGlow: {
          "0%, 100%": { boxShadow: "0 0 20px rgba(109, 93, 252, 0.3)" },
          "50%": { boxShadow: "0 0 40px rgba(109, 93, 252, 0.6)" },
        },
        waveform: {
          "0%, 100%": { transform: "scaleY(0.3)" },
          "50%": { transform: "scaleY(1.0)" },
        },
        float: {
          "0%, 100%": { transform: "translateY(0px)" },
          "50%": { transform: "translateY(-4px)" },
        },
        fadeIn: {
          "0%": { opacity: "0", transform: "translateY(8px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        scaleIn: {
          "0%": { opacity: "0", transform: "scale(0.92)" },
          "100%": { opacity: "1", transform: "scale(1)" },
        },
      },
    },
  },
  plugins: [],
};