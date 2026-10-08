Feature: Aiming down the sights

  Background:
    Given I am in test room 1
    And I have the smg

  Scenario: Right click raises the gun to its sights
    When I right click
    Then the gun is on its sights

  Scenario: Right click again lowers it
    Given I right click
    And the gun is on its sights
    When I right click
    Then the gun is back at the hip

  Scenario: Aiming with the cursor released does nothing
    Given the cursor is released
    When I right click
    Then the gun is back at the hip

  Scenario: Releasing the cursor while aiming lowers the gun
    Given I right click
    And the gun is on its sights
    When the cursor is released
    Then the gun is back at the hip

  Scenario: A shot from the hip leaves the barrel, off to the right and below the view
    When I fire once
    Then the shot started to the right of and below the view and ahead of the camera

  Scenario: A shot from the sights leaves the barrel in line with the view
    Given I right click
    And the gun is on its sights
    When I fire once
    Then the shot started in line with the view

  Scenario: A shot from the sights hits the target dummy
    Given I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy has taken 25 damage
