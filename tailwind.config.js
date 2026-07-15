/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./island.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: ['-apple-system', 'BlinkMacSystemFont', '"SF Pro Display"', '"PingFang SC"', '"Hiragino Sans GB"', 'sans-serif'],
      },
    },
  },
  plugins: [],
};
