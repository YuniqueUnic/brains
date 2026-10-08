import { By, PageElement, PageElements } from "@serenity-js/web";

/** Login screen: token exchange (design §15.1). */
export const LoginScreen = {
  tokenInput: () =>
    PageElement.located(By.css('[data-testid="token-input"]')).describedAs("token input"),
  loginButton: () =>
    PageElement.located(By.css('[data-testid="login-button"]')).describedAs("login button"),
  error: () =>
    PageElement.located(By.css('[data-testid="login-error"]')).describedAs("login error"),
};
